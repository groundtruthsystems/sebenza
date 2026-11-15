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
