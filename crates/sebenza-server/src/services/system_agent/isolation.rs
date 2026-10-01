//! How a system-agent child is started (T-04, FR-17): built by the server, in
//! an empty per-item scratch directory, with an allowlisted environment that
//! never carries `SEBENZA_CONTROL_TOKEN`, read-only tools, and a permission
//! mode that is never yolo.

use crate::domain::config::SystemAgentConfig;
use crate::services::agent_stream::{StartRunInput, StreamProvider, ToolPolicy};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Never `bypassPermissions`: plan mode is read-only by construction.
pub const PERMISSION_MODE: &str = "plan";

/// Tools the agent may use. Read-only, and with an empty cwd there is nothing
/// to read without a permission prompt, which headless runs deny.
pub const ALLOWED_TOOLS: &[&str] = &["Read", "Grep", "Glob"];

/// Tools denied outright, so a policy change in the CLI cannot widen them.
pub const DISALLOWED_TOOLS: &[&str] = &[
    "Bash",
    "Edit",
    "MultiEdit",
    "Write",
    "NotebookEdit",
    "WebFetch",
    "WebSearch",
    "Task",
    "ExitPlanMode",
];

/// Variables the child may inherit: what the CLI needs to find itself, its
/// config and its model credentials, and nothing of Sebenza's.
pub const ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TERM",
    "TMPDIR",
    "TZ",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_RUNTIME_DIR",
    "CLAUDE_CONFIG_DIR",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "AWS_REGION",
    "AWS_PROFILE",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
    "https_proxy",
    "http_proxy",
    "no_proxy",
    "SSL_CERT_FILE",
    "NODE_EXTRA_CA_CERTS",
];

/// The child's environment: `parent` filtered to [`ENV_ALLOWLIST`]. Anything
/// `SEBENZA_*` is dropped even if it were ever allowlisted (TS-60).
pub fn child_env<I>(parent: I) -> HashMap<String, String>
where
    I: IntoIterator<Item = (String, String)>,
{
    todo!("phase-3-task-5: {}", parent.into_iter().count())
}

/// The read-only tool policy every job runs under.
pub fn read_only_tools() -> ToolPolicy {
    todo!("phase-3-task-5")
}

/// Create (or empty) `<root>/<draft_id>` and return it. The id must be a bare
/// alphanumeric ULID so it cannot climb out of `root`.
pub fn prepare_scratch_dir(root: &Path, draft_id: &str) -> std::io::Result<PathBuf> {
    todo!("phase-3-task-5: {} {draft_id}", root.display())
}

/// The full run input for one job, from config and the per-job parts.
pub fn build_run_input(
    config: &SystemAgentConfig,
    draft_id: &str,
    cwd: &Path,
    prompt: String,
    resume_session_id: Option<String>,
    env: HashMap<String, String>,
) -> StartRunInput {
    todo!(
        "phase-3-task-5: {config:?} {draft_id} {} {} {resume_session_id:?} {}",
        cwd.display(),
        prompt.len(),
        env.len()
    )
}

/// The `--resume` session timeout as a [`Duration`].
pub fn job_timeout(config: &SystemAgentConfig) -> Duration {
    Duration::from_secs(config.timeout_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_stream::claude_args;

    fn config() -> SystemAgentConfig {
        SystemAgentConfig {
            enabled: true,
            model: Some("claude-haiku-4-5".into()),
            ..SystemAgentConfig::default()
        }
    }

    // TS-60: the control token and every other Sebenza or unlisted variable
    // stay out of the child's environment.
    #[test]
    fn the_child_env_is_allowlisted_and_token_free() {
        let parent = [
            ("SEBENZA_CONTROL_TOKEN", "tok"),
            ("SEBENZA_WORKTREE_PATH", "/w"),
            ("GITHUB_TOKEN", "ghp_x"),
            ("AWS_SECRET_ACCESS_KEY", "s"),
            ("PATH", "/usr/bin"),
            ("HOME", "/home/op"),
            ("ANTHROPIC_API_KEY", "k"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let env = child_env(parent);
        assert!(!env.contains_key("SEBENZA_CONTROL_TOKEN"));
        assert!(env.keys().all(|k| !k.starts_with("SEBENZA_")));
        assert!(!env.contains_key("GITHUB_TOKEN"));
        assert!(!env.contains_key("AWS_SECRET_ACCESS_KEY"));
        assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin"));
        assert_eq!(env.get("HOME").map(String::as_str), Some("/home/op"));
        assert!(env.keys().all(|k| ENV_ALLOWLIST.contains(&k.as_str())));
    }

    // TS-35: read-only allowlist, never yolo, empty cwd, isolated, timed.
    #[test]
    fn the_argv_is_read_only_and_never_yolo() {
        let cwd = Path::new("/tmp/sebenza-system-agent/X");
        let input = build_run_input(
            &config(),
            "01JABC",
            cwd,
            "JOB-KIND: triage\n".into(),
            Some("sess-1".into()),
            HashMap::new(),
        );
        assert!(input.isolated);
        assert_eq!(input.timeout, Some(Duration::from_secs(120)));
        assert_eq!(input.cwd, cwd.to_string_lossy());
        assert_eq!(input.conversation_id, "system-agent:01JABC");
        assert!(input.binary.is_none(), "claude from PATH by default");

        let args = claude_args(&input);
        let after = |flag: &str| {
            let i = args.iter().position(|a| a == flag).expect(flag);
            args[i + 1].clone()
        };
        assert_eq!(after("--permission-mode"), "plan");
        assert_eq!(after("--allowedTools"), "Read,Grep,Glob");
        let denied = after("--disallowedTools");
        for tool in ["Bash", "Edit", "Write", "WebFetch"] {
            assert!(denied.split(',').any(|t| t == tool), "{tool} not denied");
        }
        assert_eq!(after("--model"), "claude-haiku-4-5");
        assert_eq!(after("-r"), "sess-1");
        assert!(args.iter().any(|a| a == "--strict-mcp-config"));
        assert!(args.iter().any(|a| a == "--append-system-prompt"));
        for yolo in [
            "bypassPermissions",
            "--dangerously-skip-permissions",
            "--yolo",
        ] {
            assert!(!args.iter().any(|a| a == yolo), "{yolo} in {args:?}");
        }
    }

    #[test]
    fn a_fresh_session_has_no_resume_flag_and_a_binary_override_is_honoured() {
        let mut cfg = config();
        cfg.binary = Some("/opt/stub-agent.sh".into());
        cfg.model = None;
        let input = build_run_input(
            &cfg,
            "01JABC",
            Path::new("/tmp/x"),
            "p".into(),
            None,
            HashMap::new(),
        );
        assert_eq!(input.binary.as_deref(), Some("/opt/stub-agent.sh"));
        let args = claude_args(&input);
        assert!(!args.iter().any(|a| a == "-r"));
        assert!(!args.iter().any(|a| a == "--model"));
    }

    #[test]
    fn the_scratch_dir_is_emptied_and_confined() {
        let root = std::env::temp_dir().join(format!("sebenza-sa-scratch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = prepare_scratch_dir(&root, "01JABCDEF").unwrap();
        assert_eq!(dir, root.join("01JABCDEF"));
        std::fs::write(dir.join("leftover"), "x").unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let again = prepare_scratch_dir(&root, "01JABCDEF").unwrap();
        assert_eq!(std::fs::read_dir(&again).unwrap().count(), 0);
        for bad in ["", "..", "../etc", "a/b", "a.b"] {
            assert!(prepare_scratch_dir(&root, bad).is_err(), "{bad:?}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
