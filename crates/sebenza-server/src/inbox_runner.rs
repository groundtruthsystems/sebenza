//! The real [`ConversionRunner`]: git, tmux and the filesystem.
//!
//! `inbox_convert` owns the sequence and its rules; this owns doing it. Every
//! method maps a failure to a plain string, because the outcome recorded
//! against the draft is read by a person, not matched on by code.

use crate::services::project_manager::ProjectApp;
use common::adapters::fs::get_worktree_storage_paths;
use common::services::inbox_convert::{
    ConversionRunner, ConversionTarget, NOTE_REL_PATH, NOTES_MANIFEST_REL_PATH, ORIGIN_REL_PATH,
    SEBENZA_INDEX_REL_PATH, WRITTEN_REL_PATHS,
};
use common::services::lifecycle_service::{CreateMode, CreateWorktreesInput};
use std::path::Path;
use std::sync::Arc;

use crate::adapters::terminal::TerminalManager;
use crate::server::{AppState, resolve_terminal_target, submit_delay_for_branch};

/// Resolves the `ProjectApp` a target names.
///
/// A conversion is cross-project, so the runner cannot be bound to one project
/// the way every other handler is. Resolution is by **path**, not by URL
/// prefix, because a draft records where a project lives rather than how it is
/// served.
pub struct ServerConversionRunner {
    state: AppState,
    terminal: Arc<TerminalManager>,
}

impl ServerConversionRunner {
    pub fn new(state: AppState) -> Self {
        let terminal = state.terminal.clone();
        Self { state, terminal }
    }

    fn project_for(&self, project_path: &str) -> Result<Arc<ProjectApp>, String> {
        self.state
            .manager
            .list()
            .into_iter()
            .find(|app| app.path == project_path)
            .ok_or_else(|| format!("no registered project at {project_path}"))
    }

    /// Absolute path of the worktree on `branch`, asked of git rather than
    /// guessed from a naming convention.
    fn worktree_path(app: &ProjectApp, branch: &str) -> Result<String, String> {
        app.git
            .list_worktrees(&app.path)
            .into_iter()
            .find(|w| w.branch.as_deref() == Some(branch))
            .map(|w| w.path)
            .ok_or_else(|| format!("worktree for {branch} not found after creation"))
    }
}

impl ConversionRunner for ServerConversionRunner {
    fn create_worktree(&self, target: &ConversionTarget) -> Result<String, String> {
        let app = self.project_for(&target.project_path)?;
        let input = CreateWorktreesInput {
            mode: Some(CreateMode::New),
            branch: Some(target.branch.clone()),
            base_branch: target.base_branch.clone(),
            // No creation prompt: the note has to land first. The prompt is
            // sent in `send_prompt`, once the worktree is furnished.
            prompt: None,
            profile: None,
            agent: target.agent_id.clone(),
            agents: None,
            env_overrides: None,
            source: None,
            oneshot: None,
        };
        app.lifecycle()
            .create_worktrees(&input)
            .map_err(|e| e.message)?;
        Self::worktree_path(&app, &target.branch)
    }

    fn write_note(&self, worktree_path: &str, body: &str) -> Result<(), String> {
        write_private(worktree_path, NOTE_REL_PATH, body)
    }

    fn write_notes_manifest(&self, worktree_path: &str, json: &str) -> Result<(), String> {
        write_private(worktree_path, NOTES_MANIFEST_REL_PATH, json)
    }

    fn exclude_note(&self, worktree_path: &str) -> Result<(), String> {
        // `info/exclude`, not `.gitignore`: .gitignore is tracked, so writing
        // to it would dirty every converted worktree and invite a conflict on
        // merge. info/exclude is never committed.
        //
        // It must be the *common* git dir. A linked worktree's own
        // `.git/worktrees/<name>/info/exclude` is not consulted by git at all —
        // writing there looks right and silently does nothing.
        let git_dir = git_common_dir_of(worktree_path)?;
        let info = Path::new(&git_dir).join("info");
        std::fs::create_dir_all(&info).map_err(|e| e.to_string())?;
        let exclude = info.join("exclude");
        let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
        let missing: Vec<&str> = WRITTEN_REL_PATHS
            .iter()
            .copied()
            .filter(|p| !existing.lines().any(|l| l.trim() == *p))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let mut next = existing;
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str("# added by sebenza inbox\n");
        for path in missing {
            next.push_str(path);
            next.push('\n');
        }
        std::fs::write(&exclude, next).map_err(|e| e.to_string())
    }

    fn record_origin(&self, worktree_path: &str, draft_id: &str) -> Result<(), String> {
        // Alongside the worktree's own metadata, so the trail reads from the
        // worktree end too, not only from the inbox.
        let paths = get_worktree_storage_paths(worktree_path);
        std::fs::create_dir_all(&paths.sebenza_dir).map_err(|e| e.to_string())?;
        let origin = Path::new(worktree_path).join(ORIGIN_REL_PATH);
        let body = serde_json::json!({ "draftId": draft_id });
        std::fs::write(&origin, body.to_string()).map_err(|e| e.to_string())
    }

    /// `target.prompt` here is the launch prompt `run_conversion_item` built
    /// (architect-first, direct, or the operator's verbatim), not the raw
    /// request text.
    fn send_prompt(&self, target: &ConversionTarget, _worktree_path: &str) -> Result<(), String> {
        let app = self.project_for(&target.project_path)?;
        let resolved = resolve_terminal_target(&app, &target.branch)?;
        let delay = submit_delay_for_branch(&app, &target.branch);
        self.terminal
            .send_prompt(&resolved.attach_target, &target.prompt, 0, None, delay)
    }

    /// The project, not the new worktree: the workspace is what the operator
    /// set up, and the architect reads it from the checkout either way.
    fn has_sebenza_workspace(&self, target: &ConversionTarget) -> bool {
        Path::new(&target.project_path)
            .join(SEBENZA_INDEX_REL_PATH)
            .is_file()
    }

    fn now(&self) -> String {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }
}

/// The repository's common git dir — `<repo>/.git`, even from inside a linked
/// worktree. This is the only `info/exclude` git reads.
fn git_common_dir_of(worktree_path: &str) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(worktree_path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Write `contents` to `<worktree>/<rel>` readable by the owner only (0600),
/// tightening a file that already exists. Notes may hold PHI (T-06).
fn write_private(worktree_path: &str, rel: &str, contents: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let path = Path::new(worktree_path).join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| e.to_string())?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    file.write_all(contents.as_bytes())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    // TS-49: the notes a conversion writes may hold PHI, so both files are
    // owner-only, including when they replace an older, wider file.
    #[test]
    fn note_files_are_written_owner_only() {
        let dir = std::env::temp_dir().join(format!("sebenza-runner-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let wt = dir.to_string_lossy().to_string();
        std::fs::create_dir_all(dir.join(".ai/sebenza")).unwrap();
        std::fs::write(dir.join(NOTE_REL_PATH), "old").unwrap();
        std::fs::set_permissions(
            dir.join(NOTE_REL_PATH),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();

        write_private(&wt, NOTE_REL_PATH, "# notes").unwrap();
        write_private(&wt, NOTES_MANIFEST_REL_PATH, "{}").unwrap();
        for rel in [NOTE_REL_PATH, NOTES_MANIFEST_REL_PATH] {
            let meta = std::fs::metadata(dir.join(rel)).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o600, "{rel}");
        }
        assert_eq!(
            std::fs::read_to_string(dir.join(NOTE_REL_PATH)).unwrap(),
            "# notes"
        );
    }
}
