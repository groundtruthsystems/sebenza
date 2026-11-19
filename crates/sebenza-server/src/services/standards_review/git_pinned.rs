//! Git plumbing for review pins (NFR-03): every child has a timeout, a
//! scrubbed `GIT_*` environment and no inherited stdin, so a hung or steered
//! git cannot stall the daemon. Callers must not hold a daemon lock across
//! these calls.
//!
//! [`Git`] runs against the repository's common git directory as a bare
//! repository, so no call can read the working tree, its index or its
//! `.gitattributes`: only objects and refs.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long one git child may run.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a git child produced no usable output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GitError {
    #[error("spawn failed: {0}")]
    Spawn(String),
    #[error("timed out")]
    Timeout,
    #[error("output over its cap")]
    OutputTooLarge,
    #[error("{0}")]
    Failed(String),
}

/// What one child run is allowed.
#[derive(Debug, Clone, Default)]
pub struct RunLimits {
    /// Wall-clock budget; [`GIT_TIMEOUT`] when `None`.
    pub timeout: Option<Duration>,
    /// Bytes fed to the child's stdin.
    pub input: Option<Vec<u8>>,
    /// Stdout beyond this many bytes fails the run with
    /// [`GitError::OutputTooLarge`].
    pub max_output: Option<usize>,
}

/// A repository opened for object-only reads.
#[derive(Debug, Clone)]
pub struct Git {
    common_dir: PathBuf,
    program: OsString,
    timeout: Duration,
}

impl Git {
    /// Open the repository that `repo` (a worktree or git dir) belongs to.
    pub fn open(repo: &Path) -> Result<Git, GitError> {
        let dir = git_text(
            repo,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .map_err(GitError::Failed)?;
        Ok(Git {
            common_dir: PathBuf::from(dir),
            program: OsString::from("git"),
            timeout: GIT_TIMEOUT,
        })
    }

    /// The same repository with a different per-call timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Git {
        self.timeout = timeout;
        self
    }

    /// The same repository driven by another executable, to simulate a
    /// misbehaving git.
    #[cfg(test)]
    pub fn with_program(mut self, program: impl Into<OsString>) -> Git {
        self.program = program.into();
        self
    }

    /// Run `git --bare args` and return its stdout.
    pub fn run(&self, args: &[&str]) -> Result<Vec<u8>, GitError> {
        self.run_limited(args, RunLimits::default())
    }

    /// [`Git::run`] under explicit limits; the call's own timeout applies
    /// when `limits.timeout` is unset.
    pub fn run_limited(&self, args: &[&str], mut limits: RunLimits) -> Result<Vec<u8>, GitError> {
        let mut cmd = Command::new(&self.program);
        cmd.args(["--bare", "--literal-pathspecs"])
            .args(args)
            .current_dir(&self.common_dir);
        scrub_git_env(&mut cmd);
        limits.timeout.get_or_insert(self.timeout);
        run_child(cmd, limits)
    }

    /// [`Git::run`], as trimmed UTF-8.
    pub fn text(&self, args: &[&str]) -> Result<String, GitError> {
        let out = self.run(args)?;
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }

    /// The full commit SHA `rev` names.
    pub fn commit_sha(&self, rev: &str) -> Result<String, GitError> {
        self.text(&["rev-parse", "--verify", "--quiet", &commit_rev(rev)])
    }
}

/// Run `git args` in `dir` and return its stdout.
pub fn git(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir);
    scrub_git_env(&mut cmd);
    run_with_timeout(cmd, GIT_TIMEOUT).map_err(|e| format!("git {}: {e}", args.join(" ")))
}

/// [`git`], as trimmed UTF-8.
pub fn git_text(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = git(dir, args)?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

/// The full commit SHA `rev` names.
pub fn rev_parse(dir: &Path, rev: &str) -> Result<String, String> {
    git_text(dir, &["rev-parse", "--verify", "--quiet", &commit_rev(rev)])
}

/// The merge-base of two commits.
pub fn merge_base(dir: &Path, a: &str, b: &str) -> Result<String, String> {
    git_text(dir, &["merge-base", a, b])
}

fn commit_rev(rev: &str) -> String {
    format!("{rev}^{{commit}}")
}

fn scrub_git_env(cmd: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
}

/// Run `cmd`, killing it after `timeout`.
pub fn run_with_timeout(cmd: Command, timeout: Duration) -> Result<Vec<u8>, String> {
    let limits = RunLimits {
        timeout: Some(timeout),
        ..RunLimits::default()
    };
    run_child(cmd, limits).map_err(|e| e.to_string())
}

/// Run `cmd` under `limits`. Stdin is fed and stdout drained on threads so
/// neither pipe can block the child; a capped stdout is closed early, which
/// ends a writer still producing.
pub fn run_child(mut cmd: Command, limits: RunLimits) -> Result<Vec<u8>, GitError> {
    let stdin = if limits.input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    let mut child = cmd
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| GitError::Spawn(e.to_string()))?;
    let feeder = child.stdin.take().zip(limits.input).map(feed);
    let cap = limits.max_output.map(|n| n as u64 + 1).unwrap_or(u64::MAX);
    let stdout = child.stdout.take().map(|p| drain(p.take(cap)));
    let stderr = child.stderr.take().map(|p| drain(p.take(64 * 1024)));
    let status = wait_until(
        &mut child,
        Instant::now() + limits.timeout.unwrap_or(GIT_TIMEOUT),
    );
    if let Some(h) = feeder {
        let _ = h.join();
    }
    let out = stdout.and_then(|h| h.join().ok()).unwrap_or_default();
    if limits.max_output.is_some_and(|max| out.len() > max) {
        return Err(GitError::OutputTooLarge);
    }
    if status?.success() {
        return Ok(out);
    }
    let err = stderr.and_then(|h| h.join().ok()).unwrap_or_default();
    Err(GitError::Failed(
        String::from_utf8_lossy(&err).trim().to_string(),
    ))
}

fn feed((mut pipe, input): (std::process::ChildStdin, Vec<u8>)) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let _ = pipe.write_all(&input);
    })
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
) -> Result<std::process::ExitStatus, GitError> {
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| GitError::Failed(e.to_string()))?
        {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(GitError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::{self as fixture, TestRepo};

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

    #[test]
    fn stdin_is_fed_to_the_child() {
        let limits = RunLimits {
            input: Some(b"fed".to_vec()),
            ..RunLimits::default()
        };
        assert_eq!(run_child(Command::new("cat"), limits).unwrap(), b"fed");
    }

    #[test]
    fn output_over_the_cap_fails_without_reading_it_all() {
        let mut cmd = Command::new("yes");
        cmd.arg("x");
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(5)),
            max_output: Some(1024),
            ..RunLimits::default()
        };
        assert_eq!(run_child(cmd, limits), Err(GitError::OutputTooLarge));
    }

    #[test]
    fn a_worktree_opens_its_common_dir() {
        let r = TestRepo::new("git-open");
        let git = Git::open(&r.worktree).unwrap();
        assert_eq!(git.commit_sha("feat").unwrap(), r.head());
        assert_eq!(git.commit_sha("main").unwrap(), r.main_head());
    }

    #[test]
    fn working_tree_attributes_are_not_read() {
        let r = TestRepo::new("git-attrs");
        fixture::write(&r.worktree, ".gitattributes", "*.rs binary\n");
        let git = Git::open(&r.worktree).unwrap();
        let diff = git.run(&["diff", "main", "feat", "--", "src.rs"]).unwrap();
        assert!(String::from_utf8_lossy(&diff).contains("+fn main() {}"));
    }
}
