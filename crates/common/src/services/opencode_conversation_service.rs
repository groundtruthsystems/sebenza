//! Read the current opencode conversation for a worktree.
//!
//! Unlike the claude and codex services, this one does not scan on-disk logs: opencode's
//! store is SQLite with an internal schema, and `opencode export <id>` is the supported
//! read path. So this service shells out.
//!
//! The session id must come from Sebenza's own record of what it launched (the plugin's
//! `session.created`, or the `sessionID` echoed by `run --format json`), because
//! `project_id` is per-repository and `opencode session list` has no directory column —
//! neither can tell you which session belongs to this worktree. `info.directory` from the
//! export is then an integrity check, not the lookup key.

use crate::adapters::opencode_session_log::{parse_export, session_belongs_to};
use crate::domain::model::WorktreeSnapshot;
use crate::services::agents_ui::{
    build_worktree_summary, AgentsUiConversationResponse, ConversationState,
};

/// Where the opencode binary may live. `~/.opencode/bin` is not a conventional directory,
/// so a bare `which("opencode")` can fail on a perfectly good install — particularly for a
/// server started from systemd/launchd, which need not inherit the login shell's PATH.
fn opencode_binary() -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let candidate = std::path::Path::new(&home).join(".opencode").join("bin").join("opencode");
        if candidate.is_file() {
            return candidate.to_string_lossy().to_string();
        }
    }
    "opencode".to_string()
}

/// Run `opencode export <id>` in `cwd`. Deliberately WITHOUT `--sanitize`, which would
/// redact the message text and tool output the transcript needs.
///
/// Blocking (spawns a process): call from `spawn_blocking`.
fn export_session(cwd: &str, session_id: &str) -> Option<String> {
    let out = std::process::Command::new(opencode_binary())
        .args(["export", session_id])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Read the conversation for `worktree`, given the session id Sebenza recorded at launch.
///
/// Returns a `pending` placeholder when there is no recorded session yet, or when the
/// export fails or belongs to a different directory — so a mis-correlated session is
/// reported as "no conversation" rather than another worktree's transcript. That is the
/// failure mode `claude_cli::latest_session`'s all-projects fallback exhibits today.
pub fn read_worktree_conversation(
    worktree: &WorktreeSnapshot,
    session_id: Option<&str>,
) -> AgentsUiConversationResponse {
    let pending = || ConversationState {
        provider: "opencode".to_string(),
        conversation_id: format!("opencode-pending:{}", worktree.path),
        cwd: worktree.path.clone(),
        running: false,
        active_turn_id: None,
        messages: Vec::new(),
    };

    let conversation = session_id
        .and_then(|id| export_session(&worktree.path, id))
        .and_then(|text| parse_export(&text))
        .filter(|session| {
            // Integrity check: an export whose directory is not this worktree means the
            // recorded id is stale or wrong. Report nothing rather than the wrong thing.
            session.directory.is_none() || session_belongs_to(session, &worktree.path)
        })
        .map(|session| ConversationState {
            provider: "opencode".to_string(),
            conversation_id: session.id,
            cwd: worktree.path.clone(),
            running: false,
            active_turn_id: None,
            messages: session.messages,
        })
        .unwrap_or_else(pending);

    AgentsUiConversationResponse {
        worktree: build_worktree_summary(worktree),
        conversation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::opencode_session_log::parse_export;

    const FIXTURE: &str = include_str!("../adapters/testdata/opencode_export.json");

    #[test]
    fn a_session_from_another_directory_is_rejected_not_rendered() {
        let session = parse_export(FIXTURE).expect("fixture parses");
        // The fixture belongs to /repo/worktrees/example-branch.
        assert!(!session_belongs_to(&session, "/some/other/worktree"));
        assert!(session_belongs_to(&session, "/repo/worktrees/example-branch"));
    }

    #[test]
    fn binary_resolution_prefers_the_opencode_install_dir_when_present() {
        // Cannot assert an absolute answer (depends on the machine), but the resolver must
        // always yield something runnable rather than an empty string.
        let bin = opencode_binary();
        assert!(!bin.is_empty());
        assert!(bin == "opencode" || bin.ends_with("/.opencode/bin/opencode"), "got {bin}");
    }
}
