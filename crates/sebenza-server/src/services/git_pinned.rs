//! Git plumbing for review pins (NFR-03): every child has a timeout, a
//! scrubbed `GIT_*` environment and no stdin, so a hung or steered git cannot
//! stall the daemon. Callers must not hold a daemon lock across these calls.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long one git child may run.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Run `git args` in `dir` and return its stdout.
pub fn git(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    run_with_timeout(cmd, GIT_TIMEOUT).map_err(|e| format!("git {}: {e}", args.join(" ")))
}

/// [`git`], as trimmed UTF-8.
pub fn git_text(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = git(dir, args)?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

/// The full commit SHA `rev` names.
pub fn rev_parse(dir: &Path, rev: &str) -> Result<String, String> {
    git_text(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ],
    )
}

/// The merge-base of two commits.
pub fn merge_base(dir: &Path, a: &str, b: &str) -> Result<String, String> {
    git_text(dir, &["merge-base", a, b])
}

/// Run `cmd`, killing it after `timeout`. Stdout is drained on a thread so a
/// large output cannot block the child.
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn failed: {e}"))?;
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let status = wait_until(&mut child, Instant::now() + timeout)?;
    let out = stdout.and_then(|h| h.join().ok()).unwrap_or_default();
    if status.success() {
        return Ok(out);
    }
    let err = stderr.and_then(|h| h.join().ok()).unwrap_or_default();
    Err(String::from_utf8_lossy(&err).trim().to_string())
}

fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        buf
    })
}

fn wait_until(
    child: &mut std::process::Child,
    deadline: Instant,
) -> Result<std::process::ExitStatus, String> {
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("timed out".to_string());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_past_its_timeout_is_killed() {
        let mut cmd = Command::new("sleep");
        cmd.arg("5");
        let started = Instant::now();
        let err = run_with_timeout(cmd, Duration::from_millis(100)).unwrap_err();
        assert_eq!(err, "timed out");
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn a_failing_child_reports_its_stderr() {
        let dir = std::env::temp_dir();
        let err = git(&dir, &["rev-parse", "--not-a-real-flag-xyz"]).unwrap_err();
        assert!(err.starts_with("git rev-parse"), "{err}");
    }

    #[test]
    fn stdout_is_returned_on_success() {
        let mut cmd = Command::new("printf");
        cmd.arg("abc");
        assert_eq!(run_with_timeout(cmd, GIT_TIMEOUT).unwrap(), b"abc");
    }
}
