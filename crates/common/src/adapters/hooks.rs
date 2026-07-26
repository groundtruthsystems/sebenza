use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

pub struct RunLifecycleHookInput<'a> {
    /// "postCreate" | "preRemove" — used only in error messages.
    pub name: &'a str,
    pub command: &'a str,
    pub cwd: &'a str,
    pub env: &'a HashMap<String, String>,
}

fn direnv_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("direnv")
            .arg("version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

fn build_command(cwd: &str, command: &str) -> Vec<String> {
    if direnv_available() && Path::new(cwd).join(".envrc").exists() {
        let _ = Command::new("direnv").arg("allow").current_dir(cwd).output();
        return vec![
            "direnv".to_string(),
            "exec".to_string(),
            cwd.to_string(),
            "bash".to_string(),
            "-c".to_string(),
            command.to_string(),
        ];
    }
    vec!["bash".to_string(), "-c".to_string(), command.to_string()]
}

/// Run a lifecycle hook, returning `Err` with the captured output if it exits
/// non-zero. The hook inherits the process env plus the worktree runtime env.
pub fn run_lifecycle_hook(input: RunLifecycleHookInput) -> Result<(), String> {
    let argv = build_command(input.cwd, input.command);
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).current_dir(input.cwd);
    for (key, value) in input.env {
        command.env(key, value);
    }

    let output = command
        .output()
        .map_err(|e| format!("{} hook failed to spawn: {e}", input.name))?;

    if output.status.success() {
        return Ok(());
    }

    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    if detail.is_empty() {
        Err(format!("{} hook failed (exit {code})", input.name))
    } else {
        Err(format!("{} hook failed (exit {code}): {detail}", input.name))
    }
}
