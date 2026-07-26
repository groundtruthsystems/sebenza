use crate::domain::config::PaneSplit;
use sha1::{Digest, Sha1};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxWindowSummary {
    pub session_name: String,
    pub window_name: String,
    pub pane_count: i32,
}

/// Environment keys leaked from the launch project's `.env` that must be stripped
/// from tmux spawns (mirrors `SEBENZA_PROJECT_ENV_KEYS` / `stripProjectEnv`).
fn leaked_project_env_keys() -> Vec<String> {
    std::env::var("SEBENZA_PROJECT_ENV_KEYS")
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Run a tmux command, stripping the launch project's `.env` keys from the child
/// environment so the tmux server isn't born with leaked secrets.
pub fn run_tmux(args: &[&str]) -> (String, String, i32) {
    let mut command = Command::new("tmux");
    command.args(args);
    for key in leaked_project_env_keys() {
        command.env_remove(key);
    }
    match command.output() {
        Ok(output) => (
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
            output.status.code().unwrap_or(-1),
        ),
        Err(e) => (String::new(), e.to_string(), -1),
    }
}

pub fn sanitize_tmux_name_segment(value: &str, max_length: usize) -> String {
    let lowered = value.to_lowercase();
    // Replace any run of chars outside [a-z0-9_.-] with a single '-'.
    let mut replaced = String::with_capacity(lowered.len());
    let mut prev_dash = false;
    for ch in lowered.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '.' | '-') {
            replaced.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            replaced.push('-');
            prev_dash = true;
        }
    }
    // Collapse runs of '-' (runs from allowed literal dashes).
    let mut collapsed = String::with_capacity(replaced.len());
    let mut last_dash = false;
    for ch in replaced.chars() {
        if ch == '-' {
            if !last_dash {
                collapsed.push(ch);
            }
            last_dash = true;
        } else {
            collapsed.push(ch);
            last_dash = false;
        }
    }
    let trimmed: String = collapsed.trim_matches(|c| c == '.' || c == '-').to_string();
    let sliced: String = trimmed.chars().take(max_length).collect();
    if sliced.is_empty() {
        "x".to_string()
    } else {
        sliced
    }
}

pub fn build_project_session_name(project_root: &str) -> String {
    let resolved = std::fs::canonicalize(project_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| project_root.to_string());
    let base_name = Path::new(&resolved)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let base = sanitize_tmux_name_segment(&base_name, 18);
    let mut hasher = Sha1::new();
    hasher.update(resolved.as_bytes());
    let hash = hex::encode(hasher.finalize());
    format!("sebenza-{base}-{}", &hash[..8])
}

pub fn build_worktree_window_name(branch: &str) -> String {
    format!("sebenza-{branch}")
}

/// Hidden window that holds a worktree's parked (inactive) tab panes.
pub fn build_worktree_parking_window_name(branch: &str) -> String {
    format!("sebenza-{branch}-tabs")
}

/// Run a tmux command, returning its stdout or an `Err` describing the failure.
fn assert_tmux_ok(args: &[&str], action: &str) -> Result<String, String> {
    let (stdout, stderr, code) = run_tmux(args);
    if code != 0 {
        let detail = if stderr.is_empty() {
            format!("tmux {} exit {code}", args.join(" "))
        } else {
            stderr
        };
        return Err(format!("{action} failed: {detail}"));
    }
    Ok(stdout)
}

fn is_ignorable_kill_window_error(stderr: &str) -> bool {
    stderr.contains("can't find window")
        || stderr.contains("can't find session")
        || stderr.contains("no server running")
        || (stderr.contains("error connecting to") && stderr.contains("No such file or directory"))
}

/// Whether the global tmux env has already been scrubbed of leaked project keys
/// this process (see `scrub_leaked_global_env`).
static GLOBAL_ENV_SCRUBBED: AtomicBool = AtomicBool::new(false);

pub fn parse_window_summaries(output: &str) -> Vec<TmuxWindowSummary> {
    output
        .split('\n')
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut parts = line.split('\t');
            let session_name = parts.next().unwrap_or("").to_string();
            let window_name = parts.next().unwrap_or("").to_string();
            let pane_count = parts.next().unwrap_or("0").parse::<i32>().unwrap_or(0);
            TmuxWindowSummary {
                session_name,
                window_name,
                pane_count,
            }
        })
        .filter(|entry| !entry.session_name.is_empty() && !entry.window_name.is_empty())
        .collect()
}

#[derive(Clone, Default)]
pub struct TmuxGateway;

impl TmuxGateway {
    pub fn new() -> Self {
        TmuxGateway
    }

    /// List all tmux windows across sessions. Returns `Err` if tmux fails (e.g.
    /// no server running) so the caller can treat it as "no windows".
    pub fn list_windows(&self) -> Result<Vec<TmuxWindowSummary>, String> {
        let (stdout, stderr, code) = run_tmux(&[
            "list-windows",
            "-a",
            "-F",
            "#{session_name}\t#{window_name}\t#{window_panes}",
        ]);
        if code != 0 {
            return Err(stderr);
        }
        Ok(parse_window_summaries(&stdout))
    }

    pub fn ensure_server(&self) -> Result<(), String> {
        assert_tmux_ok(&["start-server"], "tmux start-server")?;
        Ok(())
    }

    /// Ensure a session exists with `destroy-unattached off` so it survives every
    /// client detaching. Creates it (rooted at `cwd`) if absent.
    pub fn ensure_session(&self, session_name: &str, cwd: &str) -> Result<(), String> {
        let (_o, _e, code) = run_tmux(&["has-session", "-t", session_name]);
        if code != 0 {
            assert_tmux_ok(
                &[
                    "new-session", "-d", "-s", session_name, "-c", cwd, ";", "set-option", "-t",
                    session_name, "destroy-unattached", "off",
                ],
                &format!("create tmux session {session_name}"),
            )?;
            self.scrub_leaked_global_env();
            return Ok(());
        }
        assert_tmux_ok(
            &["set-option", "-t", session_name, "destroy-unattached", "off"],
            &format!("set destroy-unattached off for {session_name}"),
        )?;
        self.scrub_leaked_global_env();
        Ok(())
    }

    /// Self-heal a tmux server started (by an older Sebenza) with the launch
    /// project's `.env` keys in its global env. Runs at most once per process.
    fn scrub_leaked_global_env(&self) {
        if GLOBAL_ENV_SCRUBBED.swap(true, Ordering::Relaxed) {
            return;
        }
        for key in leaked_project_env_keys() {
            run_tmux(&["set-environment", "-gu", &key]);
        }
    }

    pub fn has_window(&self, session_name: &str, window_name: &str) -> bool {
        let (stdout, _e, code) =
            run_tmux(&["list-windows", "-t", session_name, "-F", "#{window_name}"]);
        if code != 0 {
            return false;
        }
        stdout.lines().any(|line| line.trim() == window_name)
    }

    pub fn kill_window(&self, session_name: &str, window_name: &str) -> Result<(), String> {
        let target = format!("{session_name}:{window_name}");
        let (_o, stderr, code) = run_tmux(&["kill-window", "-t", &target]);
        if code != 0 && !is_ignorable_kill_window_error(&stderr) {
            return Err(format!("kill tmux window {target} failed: {stderr}"));
        }
        Ok(())
    }

    pub fn create_window(
        &self,
        session_name: &str,
        window_name: &str,
        cwd: &str,
        command: Option<&str>,
    ) -> Result<(), String> {
        let mut args = vec!["new-window", "-d", "-t", session_name, "-n", window_name, "-c", cwd];
        if let Some(cmd) = command {
            args.push(cmd);
        }
        assert_tmux_ok(&args, &format!("create tmux window {session_name}:{window_name}"))?;
        Ok(())
    }

    pub fn split_window(
        &self,
        target: &str,
        split: PaneSplit,
        size_pct: Option<i32>,
        cwd: &str,
        command: Option<&str>,
    ) -> Result<(), String> {
        let flag = if split == PaneSplit::Right { "-h" } else { "-v" };
        let mut args = vec!["split-window", "-t", target, flag, "-c", cwd];
        let size = size_pct.map(|pct| format!("{pct}%"));
        if let Some(size) = &size {
            args.push("-l");
            args.push(size);
        }
        if let Some(cmd) = command {
            args.push(cmd);
        }
        assert_tmux_ok(&args, &format!("split tmux window at {target}"))?;
        Ok(())
    }

    pub fn set_window_option(
        &self,
        session_name: &str,
        window_name: &str,
        option: &str,
        value: &str,
    ) -> Result<(), String> {
        let target = format!("{session_name}:{window_name}");
        assert_tmux_ok(
            &["set-window-option", "-t", &target, option, value],
            &format!("set tmux option {option} on {target}"),
        )?;
        Ok(())
    }

    /// Type a command into a pane and submit it (`send-keys -l` then `C-m`).
    pub fn run_command(&self, target: &str, command: &str) -> Result<(), String> {
        assert_tmux_ok(
            &["send-keys", "-t", target, "-l", "--", command],
            &format!("send tmux command to {target}"),
        )?;
        assert_tmux_ok(
            &["send-keys", "-t", target, "C-m"],
            &format!("submit tmux command on {target}"),
        )?;
        Ok(())
    }

    pub fn select_pane(&self, target: &str) -> Result<(), String> {
        assert_tmux_ok(&["select-pane", "-t", target], &format!("select tmux pane {target}"))?;
        Ok(())
    }

    /// Resolve the tmux pane id (`%N`) currently occupying a target.
    pub fn get_pane_id(&self, target: &str) -> Result<String, String> {
        assert_tmux_ok(
            &["display-message", "-p", "-t", target, "#{pane_id}"],
            &format!("resolve tmux pane id for {target}"),
        )
    }

    /// Create a detached "parked" pane holding a tab's session off-screen,
    /// returning its pane id. Creates the parking window on first use.
    pub fn create_parked_pane(
        &self,
        session_name: &str,
        parking_window: &str,
        cwd: &str,
        command: &str,
    ) -> Result<String, String> {
        if !self.has_window(session_name, parking_window) {
            return assert_tmux_ok(
                &[
                    "new-window", "-d", "-P", "-F", "#{pane_id}", "-t", session_name, "-n",
                    parking_window, "-c", cwd, command,
                ],
                &format!("create parking window {session_name}:{parking_window}"),
            );
        }
        let target = format!("{session_name}:{parking_window}");
        assert_tmux_ok(
            &["split-window", "-d", "-P", "-F", "#{pane_id}", "-t", &target, "-c", cwd, command],
            &format!("create parked pane in {target}"),
        )
    }

    /// Exchange the contents of two panes in place.
    pub fn swap_panes(&self, source: &str, destination: &str) -> Result<(), String> {
        assert_tmux_ok(
            &["swap-pane", "-s", source, "-t", destination],
            &format!("swap tmux panes {source} <-> {destination}"),
        )?;
        Ok(())
    }

    /// Remove a pane, tolerating an already-gone pane.
    pub fn kill_pane(&self, target: &str) -> Result<(), String> {
        let (_o, stderr, code) = run_tmux(&["kill-pane", "-t", target]);
        if code != 0 && !stderr.contains("can't find pane") && !is_ignorable_kill_window_error(&stderr)
        {
            return Err(format!("kill tmux pane {target} failed: {stderr}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tab_separated_summaries() {
        let output = "sess\tsebenza-main\t2\nsess\tsebenza-feature\t1\n";
        let parsed = parse_window_summaries(output);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].session_name, "sess");
        assert_eq!(parsed[0].window_name, "sebenza-main");
        assert_eq!(parsed[0].pane_count, 2);
        assert_eq!(parsed[1].pane_count, 1);
    }

    #[test]
    fn drops_rows_with_empty_session_or_window() {
        // Blank/whitespace lines dropped; a row with no window field ("lonely" has
        // no tab) is dropped; a valid row with a non-numeric pane count falls to 0.
        let output = "sess\twin\t2\n\n  \nlonely\nsess\twin2\tnan\n";
        let parsed = parse_window_summaries(output);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].pane_count, 2);
        assert_eq!(parsed[1].window_name, "win2");
        // Non-numeric pane count falls back to 0.
        assert_eq!(parsed[1].pane_count, 0);
    }

    #[test]
    fn session_name_has_prefix_and_hash() {
        let name = build_project_session_name("/nonexistent/path/webmux");
        assert!(name.starts_with("sebenza-webmux-"));
        // sebenza- + base + - + 8 hex chars
        assert_eq!(name.rsplit('-').next().unwrap().len(), 8);
    }

    #[test]
    fn sanitize_collapses_and_trims() {
        assert_eq!(sanitize_tmux_name_segment("My Repo!!Name", 18), "my-repo-name");
        assert_eq!(sanitize_tmux_name_segment("--x--", 18), "x");
        assert_eq!(sanitize_tmux_name_segment("", 18), "x");
    }
}
