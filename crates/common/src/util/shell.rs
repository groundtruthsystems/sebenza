use std::path::Path;
use std::process::Command;

/// Result of a synchronous command execution, mirroring `backend-legacy/src/lib/shell.ts`.
pub struct RunResult {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Run a command synchronously and capture its output. Optionally set the working
/// directory. Env/timeout are not threaded (the legacy helper doesn't either).
pub fn run(cmd: &str, args: &[&str], cwd: Option<&Path>) -> RunResult {
    let mut command = Command::new(cmd);
    command.args(args);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    match command.output() {
        Ok(output) => RunResult {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: output.stderr,
        },
        Err(_) => RunResult {
            success: false,
            stdout: Vec::new(),
            stderr: Vec::new(),
        },
    }
}

/// Returns true if `tool` is resolvable on PATH.
pub fn which(tool: &str) -> bool {
    run("which", &[tool], None).success
}

/// The project display name: `package.json`'s `name`, else the git-root basename.
/// Mirrors `detectProjectName`.
pub fn detect_project_name(git_root: &str) -> String {
    let basename = || {
        Path::new(git_root)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| git_root.to_string())
    };
    let pkg_path = Path::new(git_root).join("package.json");
    let Ok(raw) = std::fs::read_to_string(&pkg_path) else {
        return basename();
    };
    serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(basename)
}
