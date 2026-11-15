//! The dismissal-notice dispatcher (track sebenza_polishing, FR-08). One
//! tokio task per daemon: on start it rebuilds pending notices from the event
//! log (they are folded, not stored), then every `poll_secs` sends the ones
//! whose backoff has elapsed. Sends paste through tmux, so each pass runs on
//! the blocking pool. Notice outcomes never touch request status.

use crate::services::inbox_service::InboxService;
use std::sync::Arc;
use std::time::Duration;

/// Run one pass now, off the async runtime. Used after a dismissal so its
/// notice goes out without waiting for the next tick.
pub fn kick(inbox: Arc<InboxService>) {
    tokio::task::spawn_blocking(move || pass(&inbox));
}

fn pass(inbox: &InboxService) {
    match inbox.dispatch_due_notices(chrono::Utc::now()) {
        Ok(0) => {}
        Ok(n) => tracing::info!(
            attempts = n,
            pending = inbox.pending_notice_count(),
            "inbox: notice dispatcher pass"
        ),
        Err(e) => tracing::warn!("inbox: notice dispatcher pass failed: {e}"),
    }
}

/// Start the dispatcher loop. The first pass runs immediately: that is the
/// startup rebuild, resuming any notice a restart interrupted.
pub fn spawn(inbox: Arc<InboxService>, poll_secs: u64) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(poll_secs.max(1)));
        loop {
            tick.tick().await;
            let inbox = inbox.clone();
            let _ = tokio::task::spawn_blocking(move || pass(&inbox)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::inbox_store::InboxStore;
    use crate::adapters::projects_registry::ProjectsRegistry;

    fn service(dir: &std::path::Path) -> Arc<InboxService> {
        Arc::new(InboxService::new(
            InboxStore::with_dir(dir.join("inbox")),
            ProjectsRegistry::with_file(dir.join("projects.json")),
        ))
    }

    fn temp(tag: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("sebenza-dispatcher-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // A pass with nothing pending, and a pass over an unreadable store, both
    // return quietly: the loop logs and keeps ticking, it never panics.
    #[tokio::test]
    async fn passes_survive_an_empty_and_a_broken_store() {
        let empty = temp("empty");
        let inbox = service(&empty);
        pass(&inbox);
        assert_eq!(inbox.pending_notice_count(), 0);

        let broken = temp("broken");
        std::fs::write(broken.join("inbox"), "not a directory").unwrap();
        let inbox = service(&broken);
        pass(&inbox);

        // The loop's first tick is immediate (the startup rebuild); kick runs
        // one pass off the runtime. Neither may take the runtime down.
        spawn(inbox.clone(), 1);
        kick(inbox.clone());
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(inbox.pending_notice_count(), 0);
    }
}
