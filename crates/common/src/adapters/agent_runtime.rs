use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// The exact `sebenza-agentctl` Python helper.
const AGENTCTL_SCRIPT: &str = include_str!("testdata/sebenza-agentctl.py");

const GENERATED_CODEX_HOOKS_EXCLUDE: &str = ".codex/hooks.json";

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// `<agentctl> <subcommand>` command string for a hook entry.
fn cmd(agentctl: &str, sub: &str) -> String {
    format!("{} {sub}", shell_quote(agentctl))
}

fn claude_hook_settings(agentctl: &str) -> Value {
    json!({
        "UserPromptSubmit": [{"hooks": [{"type": "command", "command": cmd(agentctl, "claude-user-prompt-submit"), "async": true}]}],
        "Notification": [{"matcher": "permission_prompt|elicitation_dialog", "hooks": [{"type": "command", "command": cmd(agentctl, "status-changed --lifecycle idle"), "async": true}]}],
        "Stop": [{"hooks": [{"type": "command", "command": cmd(agentctl, "agent-stopped"), "async": true}]}],
        "PostToolUse": [
            {"hooks": [{"type": "command", "command": cmd(agentctl, "status-changed --lifecycle running"), "async": true}]},
            {"matcher": "Bash", "hooks": [{"type": "command", "command": cmd(agentctl, "claude-post-tool-use"), "async": true}]}
        ]
    })
}

fn codex_hook_settings(agentctl: &str) -> Value {
    json!({
        "SessionStart": [{"matcher": "startup|resume|clear", "hooks": [{"type": "command", "command": cmd(agentctl, "codex-session-start"), "timeout": 30}]}],
        "UserPromptSubmit": [{"hooks": [{"type": "command", "command": cmd(agentctl, "codex-user-prompt-submit"), "timeout": 30}]}],
        "PermissionRequest": [{"hooks": [{"type": "command", "command": cmd(agentctl, "codex-permission-request"), "timeout": 30}]}],
        "PreToolUse": [{"hooks": [{"type": "command", "command": cmd(agentctl, "status-changed --lifecycle running --best-effort"), "timeout": 30}]}],
        "PostToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": cmd(agentctl, "codex-post-tool-use"), "timeout": 30}]}],
        "Stop": [{"hooks": [{"type": "command", "command": cmd(agentctl, "codex-stop"), "timeout": 30}]}]
    })
}

/// Write the agentctl helper + Claude/Codex hook configs for a worktree.
pub fn ensure_agent_runtime_artifacts(git_dir: &str, worktree_path: &str) -> Result<(), String> {
    let sebenza_dir = Path::new(git_dir).join(".ai").join("sebenza");
    fs::create_dir_all(&sebenza_dir).map_err(|e| e.to_string())?;
    let agentctl_path = sebenza_dir.join("sebenza-agentctl");
    fs::write(&agentctl_path, AGENTCTL_SCRIPT).map_err(|e| e.to_string())?;
    set_executable(&agentctl_path);
    let agentctl = agentctl_path.to_string_lossy().to_string();

    let claude_settings = Path::new(worktree_path).join(".claude").join("settings.local.json");
    merge_claude_settings(&claude_settings, &claude_hook_settings(&agentctl))?;

    let codex_hooks = Path::new(worktree_path).join(".codex").join("hooks.json");
    merge_codex_hooks(&codex_hooks, &codex_hook_settings(&agentctl), &agentctl)?;
    ensure_generated_codex_hooks_ignored(git_dir)?;
    Ok(())
}

fn read_json_object(path: &Path) -> serde_json::Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, format!("{json}\n")).map_err(|e| e.to_string())
}

/// Claude: our hook events overwrite same-named events, other keys preserved.
fn merge_claude_settings(path: &Path, hooks: &Value) -> Result<(), String> {
    let mut existing = read_json_object(path);
    let mut merged_hooks = existing
        .get("hooks")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(hooks_obj) = hooks.as_object() {
        for (k, v) in hooks_obj {
            merged_hooks.insert(k.clone(), v.clone());
        }
    }
    existing.insert("hooks".to_string(), Value::Object(merged_hooks));
    write_json(path, &Value::Object(existing))
}

fn command_starts_with_agentctl(command: &str, agentctl: &str) -> bool {
    let c = command.trim_start();
    let quoted = shell_quote(agentctl);
    c == agentctl
        || c.starts_with(&format!("{agentctl} "))
        || c == quoted
        || c.starts_with(&format!("{quoted} "))
}

fn is_sebenza_hook_group(group: &Value, agentctl: &str) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .map(|hooks| {
            hooks.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .map(|c| command_starts_with_agentctl(c, agentctl))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Codex: per event, drop prior Sebenza groups and append the fresh ones.
fn merge_codex_hooks(path: &Path, hooks: &Value, agentctl: &str) -> Result<(), String> {
    let mut existing = read_json_object(path);
    let existing_hooks = existing.get("hooks").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut merged = existing_hooks.clone();
    if let Some(hooks_obj) = hooks.as_object() {
        for (event, groups) in hooks_obj {
            let mut preserved: Vec<Value> = existing_hooks
                .get(event)
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter(|g| !is_sebenza_hook_group(g, agentctl)).cloned().collect())
                .unwrap_or_default();
            if let Some(new_groups) = groups.as_array() {
                preserved.extend(new_groups.iter().cloned());
            }
            merged.insert(event.clone(), Value::Array(preserved));
        }
    }
    existing.insert("hooks".to_string(), Value::Object(merged));
    write_json(path, &Value::Object(existing))
}

fn resolve_git_common_dir(git_dir: &str) -> PathBuf {
    match fs::read_to_string(Path::new(git_dir).join("commondir")) {
        Ok(content) => {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                PathBuf::from(git_dir)
            } else if trimmed.starts_with('/') {
                PathBuf::from(trimmed)
            } else {
                Path::new(git_dir).join(trimmed)
            }
        }
        Err(_) => PathBuf::from(git_dir),
    }
}

/// Add the generated `.codex/hooks.json` to the repo's `info/exclude`.
fn ensure_generated_codex_hooks_ignored(git_dir: &str) -> Result<(), String> {
    let exclude_path = resolve_git_common_dir(git_dir).join("info").join("exclude");
    let existing = fs::read_to_string(&exclude_path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == GENERATED_CODEX_HOOKS_EXCLUDE) {
        return Ok(());
    }
    if let Some(parent) = exclude_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let separator = if !existing.is_empty() && !existing.ends_with('\n') { "\n" } else { "" };
    fs::write(&exclude_path, format!("{existing}{separator}{GENERATED_CODEX_HOOKS_EXCLUDE}\n"))
        .map_err(|e| e.to_string())
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::id::random_hex;

    #[test]
    fn writes_agentctl_and_hooks_idempotently() {
        let base = std::env::temp_dir().join(format!("sebenza-artifacts-{}", random_hex(4)));
        let git_dir = base.join("git");
        let wt = base.join("wt");
        fs::create_dir_all(&git_dir).unwrap();
        fs::create_dir_all(&wt).unwrap();
        let git_dir = git_dir.to_string_lossy().to_string();
        let wt = wt.to_string_lossy().to_string();

        ensure_agent_runtime_artifacts(&git_dir, &wt).unwrap();

        // agentctl written verbatim.
        let ctl = fs::read_to_string(Path::new(&git_dir).join(".ai").join("sebenza").join("sebenza-agentctl")).unwrap();
        assert!(ctl.starts_with("#!/usr/bin/env python3"));
        assert_eq!(ctl, AGENTCTL_SCRIPT);

        let claude: Value = serde_json::from_str(
            &fs::read_to_string(Path::new(&wt).join(".claude").join("settings.local.json")).unwrap(),
        )
        .unwrap();
        let stop_cmd = claude["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
        assert!(stop_cmd.contains("agent-stopped"));

        let codex: Value = serde_json::from_str(
            &fs::read_to_string(Path::new(&wt).join(".codex").join("hooks.json")).unwrap(),
        )
        .unwrap();
        assert!(codex["hooks"]["SessionStart"].is_array());

        // Re-running must not duplicate the Sebenza codex hook groups.
        ensure_agent_runtime_artifacts(&git_dir, &wt).unwrap();
        let codex2: Value = serde_json::from_str(
            &fs::read_to_string(Path::new(&wt).join(".codex").join("hooks.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(codex2["hooks"]["Stop"].as_array().unwrap().len(), 1);

        // The generated codex hooks file is git-excluded.
        let exclude = fs::read_to_string(Path::new(&git_dir).join("info").join("exclude")).unwrap();
        assert!(exclude.contains(".codex/hooks.json"));

        fs::remove_dir_all(&base).ok();
    }
}
