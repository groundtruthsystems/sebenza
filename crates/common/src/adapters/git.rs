use std::fs;
use std::path::Path;
use std::process::Command;

/// A worktree entry parsed from `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktreeEntry {
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub detached: bool,
    pub bare: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktreeStatus {
    pub dirty: bool,
    pub ahead_count: i32,
    pub current_commit: Option<String>,
}

/// Result of a git command: `Ok(stdout)` (trimmed) or `Err(stderr)`.
pub type TryGit = Result<String, String>;

/// Spawn git and return the trimmed stdout, or an error string. Catches the
/// spawn failure that occurs when `cwd` doesn't exist (matches the TS adapter).
fn spawn_git(args: &[&str], cwd: &str) -> Result<std::process::Output, String> {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("spawn error (cwd={cwd}): {e}"))
}

/// Throwing variant: returns trimmed stdout, or an `Err` with a descriptive message.
fn run_git(args: &[&str], cwd: &str) -> Result<String, String> {
    let output = spawn_git(args, cwd).map_err(|e| format!("git {} failed: {e}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            format!("exit {}", output.status.code().unwrap_or(-1))
        } else {
            stderr
        };
        return Err(format!("git {} failed: {detail}", args.join(" ")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Non-throwing Result variant: `Ok(stdout)` on success, `Err(stderr)` otherwise.
fn try_run_git(args: &[&str], cwd: &str) -> TryGit {
    let output = spawn_git(args, cwd)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Pure parser for `git worktree list --porcelain` output.
pub fn parse_git_worktree_porcelain(output: &str) -> Vec<GitWorktreeEntry> {
    let mut entries: Vec<GitWorktreeEntry> = Vec::new();
    let mut current: Option<GitWorktreeEntry> = None;

    fn flush(current: &mut Option<GitWorktreeEntry>, entries: &mut Vec<GitWorktreeEntry>) {
        if let Some(entry) = current.take()
            && !entry.path.is_empty()
        {
            entries.push(entry);
        }
    }

    for raw_line in output.split('\n') {
        let line = raw_line.trim_end();
        if line.is_empty() {
            flush(&mut current, &mut entries);
            continue;
        }

        if let Some(path) = line.strip_prefix("worktree ") {
            flush(&mut current, &mut entries);
            current = Some(GitWorktreeEntry {
                path: path.to_string(),
                branch: None,
                head: None,
                detached: false,
                bare: false,
            });
            continue;
        }

        let Some(entry) = current.as_mut() else {
            continue;
        };

        if let Some(branch) = line.strip_prefix("branch ") {
            entry.branch = Some(branch.strip_prefix("refs/heads/").unwrap_or(branch).to_string());
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            entry.head = Some(head.to_string());
        } else if line == "detached" {
            entry.detached = true;
        } else if line == "bare" {
            entry.bare = true;
        }
    }

    flush(&mut current, &mut entries);
    entries
}

fn worktree_entry_path_exists(entry: &GitWorktreeEntry) -> bool {
    fs::metadata(&entry.path)
        .map(|m| m.is_dir())
        .unwrap_or(false)
}

/// Stateless git gateway. All methods shell out per-call (matching the TS adapter).
#[derive(Clone, Default)]
pub struct GitGateway;

impl GitGateway {
    pub fn new() -> Self {
        GitGateway
    }

    /// Resolve the git repo root for `dir`. If `dir` isn't a repo, scan immediate
    /// children for a worktree and resolve the toplevel from there. `None` if no repo.
    pub fn resolve_repo_root(&self, dir: &str) -> Option<String> {
        if let Ok(top) = try_run_git(&["rev-parse", "--show-toplevel"], dir) {
            return Some(resolve_against(dir, &top));
        }
        let read = fs::read_dir(dir).ok()?;
        for entry in read.flatten() {
            let child = entry.path();
            if !child.is_dir() {
                continue;
            }
            let child_str = child.to_string_lossy();
            if let Ok(top) = try_run_git(&["rev-parse", "--show-toplevel"], &child_str) {
                return Some(resolve_against(&child_str, &top));
            }
        }
        None
    }

    pub fn resolve_worktree_git_dir(&self, cwd: &str) -> Result<String, String> {
        let out = run_git(&["rev-parse", "--git-dir"], cwd)?;
        Ok(resolve_against(cwd, &out))
    }

    pub fn list_worktrees(&self, cwd: &str) -> Vec<GitWorktreeEntry> {
        match run_git(&["worktree", "list", "--porcelain"], cwd) {
            Ok(output) => parse_git_worktree_porcelain(&output),
            Err(_) => Vec::new(),
        }
    }

    pub fn list_live_worktrees(&self, cwd: &str) -> Vec<GitWorktreeEntry> {
        self.list_worktrees(cwd)
            .into_iter()
            .filter(worktree_entry_path_exists)
            .collect()
    }

    pub fn list_local_branches(&self, cwd: &str) -> Vec<String> {
        match run_git(
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
            cwd,
        ) {
            Ok(output) => output
                .split('\n')
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn list_remote_branches(&self, cwd: &str) -> Vec<String> {
        // Best-effort prune fetch; ignore failure (offline).
        let _ = run_git(&["fetch", "--prune", "origin"], cwd);
        match run_git(
            &[
                "for-each-ref",
                "--format=%(refname:short)",
                "refs/remotes/origin",
            ],
            cwd,
        ) {
            Ok(output) => output
                .split('\n')
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .map(|l| l.strip_prefix("origin/").unwrap_or(&l).to_string())
                .filter(|name| name != "HEAD" && name != "origin")
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn read_worktree_status(&self, cwd: &str) -> GitWorktreeStatus {
        let dirty_output = run_git(&["status", "--porcelain"], cwd).unwrap_or_default();
        let commit = try_run_git(&["rev-parse", "HEAD"], cwd);
        let mut ahead = try_run_git(&["rev-list", "--count", "@{upstream}..HEAD"], cwd);
        if ahead.is_err() {
            ahead = try_run_git(
                &["rev-list", "--count", "HEAD", "--not", "--remotes=origin"],
                cwd,
            );
        }

        GitWorktreeStatus {
            dirty: !dirty_output.is_empty(),
            ahead_count: ahead
                .ok()
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0),
            current_commit: commit.ok().filter(|s| !s.is_empty()),
        }
    }

    pub fn read_status(&self, cwd: &str) -> Result<String, String> {
        run_git(&["status", "--short", "--untracked-files=all"], cwd)
    }

    pub fn create_worktree(&self, opts: &CreateGitWorktreeOptions) -> Result<(), String> {
        let mut args: Vec<&str> = vec!["worktree", "add"];
        match &opts.mode {
            CreateWorktreeMode::New { base_branch } => {
                args.push("-b");
                args.push(&opts.branch);
                args.push(&opts.worktree_path);
                if let Some(base) = base_branch {
                    args.push(base);
                }
            }
            CreateWorktreeMode::Existing { start_point } => match start_point {
                Some(start) => {
                    args.push("-b");
                    args.push(&opts.branch);
                    args.push(&opts.worktree_path);
                    args.push(start);
                }
                None => {
                    args.push(&opts.worktree_path);
                    args.push(&opts.branch);
                }
            },
        }
        run_git(&args, &opts.repo_root).map(|_| ())
    }

    pub fn remove_worktree(&self, repo_root: &str, worktree_path: &str, force: bool) -> Result<(), String> {
        let mut args: Vec<&str> = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(worktree_path);

        if try_run_git(&args, repo_root).is_ok() {
            return Ok(());
        }
        let failure = format!("git {} failed", args.join(" "));

        // The remove failed. If git no longer lists the worktree, the registration
        // is already gone and we just need to clear its directory; otherwise the
        // failure is real.
        let remaining = self.list_worktrees(repo_root);
        if is_registered_worktree(&remaining, worktree_path) {
            return Err(failure);
        }
        remove_directory(worktree_path)
            .map_err(|e| format!("{failure}; cleanup failed: {e}"))
    }

    pub fn delete_branch(&self, repo_root: &str, branch: &str, force: bool) -> Result<(), String> {
        run_git(&["branch", if force { "-D" } else { "-d" }, branch], repo_root).map(|_| ())
    }

    /// Merge `source_branch` into `target_branch` with `--no-ff`, restoring the
    /// original checkout afterwards. Aborts a conflicted merge before returning.
    pub fn merge_branch(
        &self,
        repo_root: &str,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<(), String> {
        let current = current_checkout_ref(repo_root)?;
        let should_restore = current.branch.as_deref() != Some(target_branch);
        if should_restore {
            run_git(&["checkout", target_branch], repo_root)?;
        }

        let mut merge_error: Option<String> = None;
        let mut cleanup_errors: Vec<String> = Vec::new();

        if let Err(err) = run_git(&["merge", "--no-ff", "--no-edit", source_branch], repo_root) {
            merge_error = Some(err);
            if let Err(stderr) = try_run_git(&["merge", "--abort"], repo_root)
                && !stderr.is_empty()
                && !stderr.contains("MERGE_HEAD missing")
            {
                cleanup_errors.push(format!("merge abort failed: {stderr}"));
            }
        }

        if should_restore
            && let Err(stderr) = try_run_git(&["checkout", &current.reference], repo_root)
        {
            cleanup_errors.push(format!("restore checkout failed: {stderr}"));
        }

        if let Some(err) = merge_error {
            let suffix = if cleanup_errors.is_empty() {
                String::new()
            } else {
                format!("; {}", cleanup_errors.join("; "))
            };
            return Err(format!("{err}{suffix}"));
        }
        if !cleanup_errors.is_empty() {
            return Err(cleanup_errors.join("; "));
        }
        Ok(())
    }

    pub fn current_branch(&self, repo_root: &str) -> Result<String, String> {
        run_git(&["branch", "--show-current"], repo_root)
    }

    pub fn read_diff(&self, cwd: &str) -> String {
        try_run_git(&["diff", "HEAD", "--no-color"], cwd).unwrap_or_default()
    }

    pub fn fetch_branch(&self, repo_root: &str, remote: &str, branch: &str) -> TryGit {
        try_run_git(&["fetch", remote, branch], repo_root)
    }

    pub fn fast_forward_merge(&self, repo_root: &str, reference: &str) -> TryGit {
        try_run_git(&["merge", "--ff-only", reference], repo_root)
    }

    pub fn hard_reset(&self, repo_root: &str, reference: &str) -> TryGit {
        try_run_git(&["reset", "--hard", reference], repo_root)
    }

    pub fn list_unpushed_commits(&self, cwd: &str) -> Vec<UnpushedCommit> {
        let mut result = try_run_git(&["log", "--oneline", "@{upstream}..HEAD"], cwd);
        if result.is_err() {
            result = try_run_git(&["log", "--oneline", "HEAD", "--not", "--remotes=origin"], cwd);
        }
        let Ok(stdout) = result else {
            return Vec::new();
        };
        stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| match line.find(' ') {
                Some(idx) => UnpushedCommit {
                    hash: line[..idx].to_string(),
                    message: line[idx + 1..].to_string(),
                },
                None => UnpushedCommit {
                    hash: line.to_string(),
                    message: String::new(),
                },
            })
            .collect()
    }
}

/// Options for `create_worktree`, mirroring the legacy discriminated union.
pub struct CreateGitWorktreeOptions {
    pub repo_root: String,
    pub worktree_path: String,
    pub branch: String,
    pub mode: CreateWorktreeMode,
}

#[derive(Clone)]
pub enum CreateWorktreeMode {
    New { base_branch: Option<String> },
    Existing { start_point: Option<String> },
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UnpushedCommit {
    pub hash: String,
    pub message: String,
}

/// The ref/branch currently checked out at `cwd` (branch is `None` when detached).
struct CheckoutRef {
    reference: String,
    branch: Option<String>,
}

fn current_checkout_ref(cwd: &str) -> Result<CheckoutRef, String> {
    if let Ok(symbolic) = try_run_git(&["symbolic-ref", "--quiet", "--short", "HEAD"], cwd)
        && !symbolic.is_empty()
    {
        return Ok(CheckoutRef {
            reference: symbolic.clone(),
            branch: Some(symbolic),
        });
    }
    Ok(CheckoutRef {
        reference: run_git(&["rev-parse", "--verify", "HEAD"], cwd)?,
        branch: None,
    })
}

fn is_registered_worktree(entries: &[GitWorktreeEntry], worktree_path: &str) -> bool {
    let resolved = canonical_path(worktree_path);
    entries
        .iter()
        .any(|entry| canonical_path(&entry.path) == resolved)
}

fn canonical_path(path: &str) -> String {
    fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string())
}

fn remove_directory(path: &str) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        // `force: true` in the legacy `rmSync` — a missing dir is not an error.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Resolve `relative` against `base` (mirrors Node `resolve(base, relative)`),
/// canonicalizing so the output is an absolute path.
fn resolve_against(base: &str, relative: &str) -> String {
    let joined = Path::new(base).join(relative);
    fs::canonicalize(&joined)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| joined.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_managed_detached_and_bare_entries() {
        let output = "\
worktree /repo
HEAD abc123
branch refs/heads/main

worktree /repo/wt/feature
HEAD def456
branch refs/heads/feature/foo

worktree /repo/wt/detached
HEAD 999aaa
detached

worktree /repo/.bare
bare
";
        let entries = parse_git_worktree_porcelain(output);
        assert_eq!(entries.len(), 4);

        assert_eq!(entries[0].path, "/repo");
        assert_eq!(entries[0].branch.as_deref(), Some("main"));
        assert!(!entries[0].detached);

        assert_eq!(entries[1].path, "/repo/wt/feature");
        // refs/heads/ prefix is stripped, nested slashes preserved.
        assert_eq!(entries[1].branch.as_deref(), Some("feature/foo"));

        assert!(entries[2].detached);
        assert_eq!(entries[2].branch, None);

        assert!(entries[3].bare);
        assert_eq!(entries[3].path, "/repo/.bare");
    }

    #[test]
    fn drops_entries_without_a_path() {
        assert!(parse_git_worktree_porcelain("").is_empty());
        // Stray attribute lines with no preceding `worktree ` are ignored.
        assert!(parse_git_worktree_porcelain("branch refs/heads/x\nHEAD abc").is_empty());
    }
}
