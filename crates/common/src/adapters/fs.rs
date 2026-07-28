use crate::domain::model::{
    OpenSessionsState, PrEntry, WorktreeArchiveState, WorktreeMeta, WorktreeStoragePaths,
    WorktreeTab, WorktreeTabKind, OPEN_SESSIONS_STATE_VERSION, ROOT_TAB_ID,
    WORKTREE_ARCHIVE_STATE_VERSION,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub fn get_worktree_storage_paths(git_dir: &str) -> WorktreeStoragePaths {
    let sebenza_dir = Path::new(git_dir).join(".ai").join("sebenza");
    let join = |name: &str| sebenza_dir.join(name).to_string_lossy().to_string();
    WorktreeStoragePaths {
        git_dir: git_dir.to_string(),
        sebenza_dir: sebenza_dir.to_string_lossy().to_string(),
        meta_path: join("meta.json"),
        runtime_env_path: join("runtime.env"),
        control_env_path: join("control.env"),
        prs_path: join("prs.json"),
    }
}

/// Read and normalize a worktree's `meta.json`. Best-effort: returns `None` when
/// the file is missing or unparseable (worktree treated as unmanaged).
pub fn read_worktree_meta(git_dir: &str) -> Option<WorktreeMeta> {
    let paths = get_worktree_storage_paths(git_dir);
    let content = fs::read_to_string(&paths.meta_path).ok()?;
    let meta: WorktreeMeta = serde_json::from_str(&content).ok()?;
    Some(normalize_worktree_meta(meta))
}

/// Backfill a single root tab for worktrees created before tabs existed, stamp
/// the owning agent onto agent-bearing tabs written before that field existed,
/// and normalize an empty/whitespace label to `None`.
fn normalize_worktree_meta(mut meta: WorktreeMeta) -> WorktreeMeta {
    if let Some(label) = meta.label.take() {
        let trimmed = label.trim();
        if !trimmed.is_empty() {
            meta.label = Some(trimmed.to_string());
        }
    }

    let has_tabs = meta.tabs.as_ref().map(|t| !t.is_empty()).unwrap_or(false);
    if !has_tabs {
        let session_id = meta
            .conversation
            .as_ref()
            .map(|c| c.conversation_session_id().to_string());
        let root_tab = WorktreeTab {
            tab_id: ROOT_TAB_ID.to_string(),
            kind: WorktreeTabKind::Root,
            label: "Root".to_string(),
            seq: None,
            session_id,
            pane_id: None,
            agent: Some(meta.agent.clone()),
            created_at: meta.created_at.clone(),
        };
        meta.tabs = Some(vec![root_tab]);
        meta.active_tab_id = Some(ROOT_TAB_ID.to_string());
        if meta.fork_counter.is_none() {
            meta.fork_counter = Some(0);
        }
    }

    // Root and fork tabs always run the worktree's own agent, so a missing
    // `agent` on an existing tab is a pre-field file, not an absence. Stamping it
    // here means the restore path never has to guess which agent a tab belongs
    // to. Shell tabs run no agent and are deliberately left as `None`.
    if let Some(tabs) = meta.tabs.take() {
        let agent = meta.agent.clone();
        meta.tabs = Some(
            tabs.into_iter()
                .map(|mut tab| {
                    if tab.agent.is_none() && tab.kind != WorktreeTabKind::Shell {
                        tab.agent = Some(agent.clone());
                    }
                    tab
                })
                .collect(),
        );
    }

    meta
}

/// Read a worktree's `prs.json`. Returns `[]` on any failure (missing/corrupt).
pub fn read_worktree_prs(git_dir: &str) -> Vec<PrEntry> {
    let paths = get_worktree_storage_paths(git_dir);
    let Ok(content) = fs::read_to_string(&paths.prs_path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<PrEntry>>(&content).unwrap_or_default()
}

/// Build the runtime env map used to expand service URL templates. Mirrors
/// `buildRuntimeEnvMap`: dotenv < startupEnvValues < allocatedPorts < extra < SEBENZA_*.
pub fn build_runtime_env_map(
    meta: &WorktreeMeta,
    extra_env: &HashMap<String, String>,
    dotenv_values: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = HashMap::new();
    env.extend(dotenv_values.iter().map(|(k, v)| (k.clone(), v.clone())));
    env.extend(meta.startup_env_values.iter().map(|(k, v)| (k.clone(), v.clone())));
    for (k, v) in &meta.allocated_ports {
        env.insert(k.clone(), v.to_string());
    }
    env.extend(extra_env.iter().map(|(k, v)| (k.clone(), v.clone())));
    env.insert("SEBENZA_WORKTREE_ID".to_string(), meta.worktree_id.clone());
    env.insert("SEBENZA_BRANCH".to_string(), meta.branch.clone());
    env.insert("SEBENZA_PROFILE".to_string(), meta.profile.clone());
    env.insert("SEBENZA_AGENT".to_string(), meta.agent.clone());
    env.insert("SEBENZA_RUNTIME".to_string(), meta.runtime.clone());
    env
}

/// Parse a `.env`-style file into a key/value map (mirrors `parseDotenv`):
/// skips comment lines, honors an optional `export` prefix, and strips matching
/// surrounding quotes.
pub fn parse_dotenv(content: &str) -> HashMap<String, String> {
    let mut env = HashMap::new();
    for line in content.split('\n') {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = parse_dotenv_line(line) else {
            continue;
        };
        let value = if raw_value.len() >= 2
            && ((raw_value.starts_with('"') && raw_value.ends_with('"'))
                || (raw_value.starts_with('\'') && raw_value.ends_with('\'')))
        {
            raw_value[1..raw_value.len() - 1].to_string()
        } else {
            raw_value.trim_end().to_string()
        };
        env.insert(key, value);
    }
    env
}

/// Match `^\s*(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)` → (key, rest).
fn parse_dotenv_line(line: &str) -> Option<(String, &str)> {
    let rest = line.trim_start();
    let rest = rest.strip_prefix("export ").map(str::trim_start).unwrap_or(rest);
    let mut chars = rest.char_indices();
    let (_, first) = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let mut end = rest.len();
    for (idx, ch) in rest.char_indices().skip(1) {
        if !(ch.is_ascii_alphanumeric() || ch == '_') {
            end = idx;
            break;
        }
    }
    let key = &rest[..end];
    let after = rest[end..].trim_start();
    let value = after.strip_prefix('=')?.trim_start();
    Some((key.to_string(), value))
}

/// Load a worktree's `.env.local`, or an empty map if absent/unreadable.
pub fn load_dotenv_local(worktree_path: &str) -> HashMap<String, String> {
    match fs::read_to_string(Path::new(worktree_path).join(".env.local")) {
        Ok(content) => parse_dotenv(&content),
        Err(_) => HashMap::new(),
    }
}

/// Why a track file read failed, so callers can pick the right HTTP status.
#[derive(Debug)]
pub enum TrackFileError {
    /// The requested path escaped the Sebenza workspace directory.
    Traversal,
    /// No Sebenza workspace directory, or the file is absent.
    NotFound,
}

/// Resolve the Sebenza workspace directory for a worktree: `<worktree>/.ai/sebenza`,
/// the layout written by the `sebenza` Claude Code plugin (`sebenza-setup`). `None`
/// when the worktree has no workspace.
///
/// Not to be confused with [`get_worktree_storage_paths`], which uses
/// `<git_dir>/.ai/sebenza` — the same suffix under a *different* root (`.git/` or
/// `.git/worktrees/<name>/`) for this app's own runtime state. The two never collide.
pub fn resolve_tracks_dir(worktree_path: &str) -> Option<PathBuf> {
    let dir = Path::new(worktree_path).join(".ai").join("sebenza");
    dir.is_dir().then_some(dir)
}

/// Read `<worktree>/.ai/sebenza/tracks.json` (`sebenza-tracks-v1`) as parsed JSON.
/// `None` when there is no workspace, the file is absent, or it doesn't parse.
pub fn read_tracks(worktree_path: &str) -> Option<serde_json::Value> {
    let dir = resolve_tracks_dir(worktree_path)?;
    let content = fs::read_to_string(dir.join("tracks.json")).ok()?;
    serde_json::from_str(&content).ok()
}

/// Read a text file at `<workspace_dir>/<rel>` (e.g. `tracks/<id>/plan.json`) — the
/// track `design_path` / `spec_path` / `plan_path` values are relative to the
/// workspace root. Guards against path traversal: `rel` may not be absolute or
/// contain `..`, and the canonicalized target must stay within the workspace dir
/// (defends against symlink escapes).
pub fn read_track_file(worktree_path: &str, rel: &str) -> Result<String, TrackFileError> {
    let dir = resolve_tracks_dir(worktree_path).ok_or(TrackFileError::NotFound)?;
    read_track_file_in(&dir, rel)
}

/// Traversal-guarded read of `<dir>/<rel>`, where `dir` is an already-resolved
/// Sebenza workspace. Split out so the registry portfolio can reuse the guard for
/// projects that aren't worktrees of the active project.
pub fn read_track_file_in(dir: &Path, rel: &str) -> Result<String, TrackFileError> {
    let rel_path = Path::new(rel.trim_start_matches("./"));
    if rel_path.is_absolute() || rel_path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(TrackFileError::Traversal);
    }
    let canon_dir = fs::canonicalize(dir).map_err(|_| TrackFileError::NotFound)?;
    match fs::canonicalize(dir.join(rel_path)) {
        Ok(target) if target.starts_with(&canon_dir) => {
            fs::read_to_string(&target).map_err(|_| TrackFileError::NotFound)
        }
        Ok(_) => Err(TrackFileError::Traversal),
        Err(_) => Err(TrackFileError::NotFound),
    }
}

fn project_archive_state_path(git_dir: &str) -> String {
    Path::new(git_dir)
        .join(".ai").join("sebenza")
        .join("archive.json")
        .to_string_lossy()
        .to_string()
}

/// Create the `<gitDir>/.ai/sebenza` storage directory (idempotent).
pub fn ensure_worktree_storage_dirs(git_dir: &str) -> Result<WorktreeStoragePaths, String> {
    let paths = get_worktree_storage_paths(git_dir);
    fs::create_dir_all(&paths.sebenza_dir).map_err(|e| e.to_string())?;
    Ok(paths)
}

/// Write `meta.json` (pretty-printed with a trailing newline).
pub fn write_worktree_meta(git_dir: &str, meta: &WorktreeMeta) -> Result<(), String> {
    let paths = ensure_worktree_storage_dirs(git_dir)?;
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    fs::write(&paths.meta_path, format!("{json}\n")).map_err(|e| e.to_string())
}

pub fn write_worktree_prs(git_dir: &str, prs: &[PrEntry]) -> Result<(), String> {
    let paths = ensure_worktree_storage_dirs(git_dir)?;
    let json = serde_json::to_string_pretty(prs).map_err(|e| e.to_string())?;
    fs::write(&paths.prs_path, format!("{json}\n")).map_err(|e| e.to_string())
}

/// Read the project's `archive.json`. Returns an empty (versioned) state on any
/// failure or shape mismatch, mirroring `readWorktreeArchiveState`.
pub fn read_worktree_archive_state(git_dir: &str) -> WorktreeArchiveState {
    let empty = || WorktreeArchiveState {
        schema_version: WORKTREE_ARCHIVE_STATE_VERSION,
        entries: Vec::new(),
    };
    let Ok(content) = fs::read_to_string(project_archive_state_path(git_dir)) else {
        return empty();
    };
    serde_json::from_str::<WorktreeArchiveState>(&content).unwrap_or_else(|_| empty())
}

pub fn write_worktree_archive_state(
    git_dir: &str,
    state: &WorktreeArchiveState,
) -> Result<(), String> {
    ensure_worktree_storage_dirs(git_dir)?;
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    fs::write(project_archive_state_path(git_dir), format!("{json}\n")).map_err(|e| e.to_string())
}

fn project_open_sessions_state_path(git_dir: &str) -> String {
    Path::new(git_dir)
        .join(".ai").join("sebenza")
        .join("open-sessions.json")
        .to_string_lossy()
        .to_string()
}

/// Read the project's `open-sessions.json`. Empty (versioned) state on any
/// failure or shape mismatch, mirroring `readOpenSessionsState`.
pub fn read_open_sessions_state(git_dir: &str) -> OpenSessionsState {
    let empty = || OpenSessionsState {
        schema_version: OPEN_SESSIONS_STATE_VERSION,
        saved_at: String::new(),
        branches: Vec::new(),
    };
    let Ok(content) = fs::read_to_string(project_open_sessions_state_path(git_dir)) else {
        return empty();
    };
    serde_json::from_str::<OpenSessionsState>(&content).unwrap_or_else(|_| empty())
}

pub fn write_open_sessions_state(git_dir: &str, state: &OpenSessionsState) -> Result<(), String> {
    ensure_worktree_storage_dirs(git_dir)?;
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    fs::write(project_open_sessions_state_path(git_dir), format!("{json}\n")).map_err(|e| e.to_string())
}

/// Serialize an env map to a sorted `KEY=value` file, shell-quoting unsafe values.
/// Mirrors `renderEnvFile` (sort by key, trailing newline).
pub fn render_env_file(env: &HashMap<String, String>) -> String {
    let mut entries: Vec<(&String, &String)> = env.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = String::new();
    for (key, value) in entries {
        out.push_str(key);
        out.push('=');
        out.push_str(&quote_env_value(value));
        out.push('\n');
    }
    out
}

fn quote_env_value(value: &str) -> String {
    if !value.is_empty() && value.chars().all(is_safe_env_char) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Safe env-value character class `[A-Za-z0-9_./:@%+=,-]`.
fn is_safe_env_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '@' | '%' | '+' | '=' | ',' | '-')
}

pub fn write_runtime_env(git_dir: &str, env: &HashMap<String, String>) -> Result<(), String> {
    let paths = ensure_worktree_storage_dirs(git_dir)?;
    fs::write(&paths.runtime_env_path, render_env_file(env)).map_err(|e| e.to_string())
}

pub fn write_control_env(git_dir: &str, env: &HashMap<String, String>) -> Result<(), String> {
    let paths = ensure_worktree_storage_dirs(git_dir)?;
    fs::write(&paths.control_env_path, render_env_file(env)).map_err(|e| e.to_string())
}

/// Build the control env map injected into agent processes (`buildControlEnvMap`).
pub fn build_control_env_map(
    control_url: &str,
    control_token: &str,
    worktree_id: &str,
    branch: &str,
    git_dir: &str,
) -> HashMap<String, String> {
    HashMap::from([
        ("SEBENZA_CONTROL_URL".to_string(), control_url.to_string()),
        // The generated opencode plugin reads this instead of having the path compiled
        // into it, which keeps the generated file byte-identical across worktrees.
        (
            "SEBENZA_AGENTCTL".to_string(),
            format!("{git_dir}/.ai/sebenza/sebenza-agentctl"),
        ),
        ("SEBENZA_CONTROL_TOKEN".to_string(), control_token.to_string()),
        ("SEBENZA_WORKTREE_ID".to_string(), worktree_id.to_string()),
        ("SEBENZA_BRANCH".to_string(), branch.to_string()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_env_carries_the_agentctl_path_for_the_opencode_plugin() {
        let env = build_control_env_map("http://127.0.0.1:5111", "tok", "wt1", "feature", "/repo/.git");
        assert_eq!(
            env.get("SEBENZA_AGENTCTL").map(String::as_str),
            Some("/repo/.git/.ai/sebenza/sebenza-agentctl"),
            "the generated opencode plugin reads the agentctl path from the env so the \
             generated file stays byte-identical across worktrees"
        );
        // Pre-existing keys must be untouched.
        assert_eq!(env.get("SEBENZA_CONTROL_TOKEN").map(String::as_str), Some("tok"));
        assert_eq!(env.get("SEBENZA_BRANCH").map(String::as_str), Some("feature"));
    }

    fn temp_worktree() -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("sebenza-tracks-test-{}", crate::util::id::random_hex(8)));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolve_tracks_dir_finds_ai_sebenza_only() {
        let wt = temp_worktree();
        assert!(resolve_tracks_dir(&wt.to_string_lossy()).is_none());

        // The retired Conductor layouts are no longer recognized.
        fs::create_dir_all(wt.join("conductor")).unwrap();
        fs::create_dir_all(wt.join(".sebenza").join("conductor")).unwrap();
        assert!(resolve_tracks_dir(&wt.to_string_lossy()).is_none());

        let workspace = wt.join(".ai").join("sebenza");
        fs::create_dir_all(&workspace).unwrap();
        assert_eq!(resolve_tracks_dir(&wt.to_string_lossy()), Some(workspace));

        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn read_tracks_present_and_absent() {
        let wt = temp_worktree();
        let wt_str = wt.to_string_lossy().to_string();
        assert!(read_tracks(&wt_str).is_none());

        let dir = wt.join(".ai").join("sebenza");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tracks.json"), r#"{"tracks":[{"track_id":"x"}]}"#).unwrap();
        let tracks = read_tracks(&wt_str).unwrap();
        assert_eq!(tracks["tracks"][0]["track_id"], "x");

        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn read_track_file_guards_traversal() {
        let wt = temp_worktree();
        let wt_str = wt.to_string_lossy().to_string();
        let dir = wt.join(".ai").join("sebenza");
        fs::create_dir_all(dir.join("tracks").join("t_1")).unwrap();
        fs::write(dir.join("tracks").join("t_1").join("plan.json"), "{}").unwrap();
        // Secret file outside the workspace dir, to attempt to reach via `..`.
        fs::write(wt.join("secret.txt"), "nope").unwrap();

        // Nested read works (leading "./" tolerated).
        assert_eq!(read_track_file(&wt_str, "./tracks/t_1/plan.json").unwrap(), "{}");
        // Traversal is rejected.
        assert!(matches!(
            read_track_file(&wt_str, "../../secret.txt"),
            Err(TrackFileError::Traversal)
        ));
        // Absent file → NotFound.
        assert!(matches!(
            read_track_file(&wt_str, "tracks/t_1/missing.md"),
            Err(TrackFileError::NotFound)
        ));

        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn env_file_is_sorted_and_quotes_unsafe_values() {
        let env = HashMap::from([
            ("B_SAFE".to_string(), "plain-value.1".to_string()),
            ("A_UNSAFE".to_string(), "has space".to_string()),
            ("C_QUOTE".to_string(), "it's".to_string()),
        ]);
        let rendered = render_env_file(&env);
        assert_eq!(
            rendered,
            "A_UNSAFE='has space'\nB_SAFE=plain-value.1\nC_QUOTE='it'\\''s'\n"
        );
    }
}

#[cfg(test)]
mod meta_normalization_tests {
    use super::*;

    /// A `meta.json` as written before per-tab agents existed: no `agent` field on
    /// any tab. Deserializing this must succeed, not fall back to "unmanaged".
    const LEGACY_META_JSON: &str = r#"{
        "schemaVersion": 1,
        "worktreeId": "wt-1",
        "branch": "feature",
        "createdAt": "2026-01-01T00:00:00Z",
        "profile": "default",
        "agent": "codex",
        "runtime": "host",
        "startupEnvValues": {},
        "allocatedPorts": {},
        "tabs": [
            { "tabId": "root", "kind": "root", "label": "Root", "seq": null,
              "sessionId": "sess-root", "createdAt": "2026-01-01T00:00:00Z" },
            { "tabId": "fork-1", "kind": "fork", "label": "Fork 1", "seq": 1,
              "sessionId": "sess-fork", "createdAt": "2026-01-01T00:00:00Z" },
            { "tabId": "shell-1", "kind": "shell", "label": "Shell", "seq": null,
              "sessionId": null, "createdAt": "2026-01-01T00:00:00Z" }
        ],
        "activeTabId": "root",
        "forkCounter": 1
    }"#;

    #[test]
    fn legacy_meta_without_tab_agents_deserializes_and_is_backfilled() {
        let raw: WorktreeMeta = serde_json::from_str(LEGACY_META_JSON).unwrap();
        // Before normalization the field is simply absent.
        assert!(raw.tabs.as_ref().unwrap().iter().all(|t| t.agent.is_none()));

        let meta = normalize_worktree_meta(raw);
        let tabs = meta.tabs.clone().unwrap();
        let by_id = |id: &str| tabs.iter().find(|t| t.tab_id == id).unwrap().clone();
        // Root and fork run the worktree's agent, so they are stamped with it...
        assert_eq!(by_id("root").agent.as_deref(), Some("codex"));
        assert_eq!(by_id("fork-1").agent.as_deref(), Some("codex"));
        // ...but a shell runs no agent and must stay unstamped.
        assert_eq!(by_id("shell-1").agent, None);
    }

    #[test]
    fn backfilled_root_tab_carries_the_worktree_agent() {
        let mut meta: WorktreeMeta = serde_json::from_str(LEGACY_META_JSON).unwrap();
        meta.tabs = None; // pre-tabs worktree
        meta.active_tab_id = None;
        meta.fork_counter = None;

        let normalized = normalize_worktree_meta(meta);
        let tabs = normalized.tabs.unwrap();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].kind, WorktreeTabKind::Root);
        assert_eq!(tabs[0].agent.as_deref(), Some("codex"));
        assert_eq!(normalized.active_tab_id.as_deref(), Some(ROOT_TAB_ID));
    }

    #[test]
    fn an_explicit_tab_agent_survives_normalization() {
        // A Codex tab on a Claude worktree must not be rewritten to "claude".
        let mut meta: WorktreeMeta = serde_json::from_str(LEGACY_META_JSON).unwrap();
        meta.agent = "claude".to_string();
        let mut tabs = meta.tabs.take().unwrap();
        tabs[1].agent = Some("codex".to_string());
        meta.tabs = Some(tabs);

        let normalized = normalize_worktree_meta(meta);
        let tabs = normalized.tabs.unwrap();
        assert_eq!(tabs[0].agent.as_deref(), Some("claude")); // backfilled
        assert_eq!(tabs[1].agent.as_deref(), Some("codex")); // preserved
    }

    #[test]
    fn agent_tab_kind_round_trips_as_camel_case() {
        let tab = WorktreeTab {
            tab_id: "agent-codex-1".to_string(),
            kind: WorktreeTabKind::Agent,
            label: "Codex".to_string(),
            seq: Some(1),
            session_id: None,
            pane_id: None,
            agent: Some("codex".to_string()),
            created_at: "t".to_string(),
        };
        let json = serde_json::to_value(&tab).unwrap();
        assert_eq!(json["kind"], "agent");
        assert_eq!(json["agent"], "codex");
        let back: WorktreeTab = serde_json::from_value(json).unwrap();
        assert_eq!(back.kind, WorktreeTabKind::Agent);
    }

    #[test]
    fn absent_optional_fields_are_omitted_from_the_wire() {
        let tab = WorktreeTab {
            tab_id: "shell-1".to_string(),
            kind: WorktreeTabKind::Shell,
            label: "Shell".to_string(),
            seq: None,
            session_id: None,
            pane_id: None,
            agent: None,
            created_at: "t".to_string(),
        };
        let json = serde_json::to_value(&tab).unwrap();
        // Matches paneId's existing `.optional()` (omitted) contract, not null.
        assert!(json.get("agent").is_none());
        assert!(json.get("paneId").is_none());
    }
}
