use crate::adapters::fs::write_worktree_prs;
use crate::adapters::git::GitGateway;
use crate::domain::config::LinkedRepoConfig;
use crate::domain::model::{CiCheck, PrComment, PrEntry};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PR_FETCH_LIMIT: usize = 50;
const GH_TIMEOUT: Duration = Duration::from_secs(15);

// ── gh JSON shapes ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GhComment {
    author: Option<GhAuthor>,
    #[serde(default)]
    body: String,
    #[serde(rename = "createdAt", default)]
    created_at: String,
}

#[derive(Deserialize)]
struct GhAuthor {
    #[serde(default)]
    login: String,
}

#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum GhCheckEntry {
    CheckRun {
        conclusion: Option<String>,
        status: String,
        name: String,
        #[serde(rename = "detailsUrl")]
        details_url: Option<String>,
    },
    StatusContext {
        context: String,
        state: String,
        #[serde(rename = "targetUrl")]
        target_url: Option<String>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct GhPrEntry {
    number: i32,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    state: String,
    #[serde(rename = "updatedAt", default)]
    updated_at: String,
    #[serde(rename = "statusCheckRollup")]
    status_check_rollup: Option<Vec<GhCheckEntry>>,
    url: String,
    #[serde(default)]
    comments: Vec<GhComment>,
}

// ── Pure helpers ──────────────────────────────────────────────────────────────

/// Summarize CI status from a statusCheckRollup: none/pending/success/failed.
fn summarize_checks(checks: &Option<Vec<GhCheckEntry>>) -> String {
    let Some(checks) = checks else {
        return "none".to_string();
    };
    if checks.is_empty() {
        return "none".to_string();
    }
    let all_done = checks.iter().all(|c| match c {
        GhCheckEntry::StatusContext { state, .. } => state != "PENDING" && state != "EXPECTED",
        GhCheckEntry::CheckRun { status, .. } => status == "COMPLETED",
        GhCheckEntry::Unknown => true,
    });
    if !all_done {
        return "pending".to_string();
    }
    let all_pass = checks.iter().all(|c| match c {
        GhCheckEntry::StatusContext { state, .. } => state == "SUCCESS",
        GhCheckEntry::CheckRun { conclusion, .. } => {
            matches!(conclusion.as_deref(), Some("SUCCESS") | Some("NEUTRAL") | Some("SKIPPED"))
        }
        GhCheckEntry::Unknown => true,
    });
    if all_pass { "success".to_string() } else { "failed".to_string() }
}

/// Parse a GitHub Actions run id from a details URL (`.../actions/runs/<id>`).
fn parse_run_id(details_url: Option<&str>) -> Option<i64> {
    let url = details_url?;
    let after = url.split("/actions/runs/").nth(1)?;
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<i64>().ok()
}

fn derive_check_status(check: &GhCheckEntry) -> String {
    match check {
        GhCheckEntry::StatusContext { state, .. } => match state.as_str() {
            "SUCCESS" => "success",
            "PENDING" | "EXPECTED" => "pending",
            _ => "failed",
        },
        GhCheckEntry::CheckRun { status, conclusion, .. } => {
            if status != "COMPLETED" {
                "pending"
            } else {
                match conclusion.as_deref() {
                    Some("SUCCESS") | Some("NEUTRAL") => "success",
                    Some("SKIPPED") => "skipped",
                    _ => "failed",
                }
            }
        }
        GhCheckEntry::Unknown => "pending",
    }
    .to_string()
}

fn map_checks(checks: &Option<Vec<GhCheckEntry>>) -> Vec<CiCheck> {
    let Some(checks) = checks else {
        return Vec::new();
    };
    checks
        .iter()
        .filter_map(|c| {
            let (name, url) = match c {
                GhCheckEntry::StatusContext { context, target_url, .. } => {
                    (context.clone(), target_url.clone())
                }
                GhCheckEntry::CheckRun { name, details_url, .. } => (name.clone(), details_url.clone()),
                GhCheckEntry::Unknown => return None,
            };
            let run_id = parse_run_id(url.as_deref());
            Some(CiCheck {
                name,
                status: derive_check_status(c),
                url,
                run_id,
            })
        })
        .collect()
}

/// Parse `gh pr list --json` output into a branch → PrEntry map (first PR per
/// branch wins). Errors on invalid JSON.
fn parse_pr_response(json: &str, repo_label: Option<&str>) -> Result<HashMap<String, PrEntry>, String> {
    let entries: Vec<GhPrEntry> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut prs = HashMap::new();
    for entry in entries {
        if prs.contains_key(&entry.head_ref_name) {
            continue;
        }
        let comments = entry
            .comments
            .iter()
            .map(|c| PrComment {
                r#type: "comment".to_string(),
                author: c.author.as_ref().map(|a| a.login.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| "unknown".to_string()),
                body: c.body.clone(),
                created_at: c.created_at.clone(),
                path: None,
                line: None,
                diff_hunk: None,
                is_reply: None,
            })
            .collect();
        prs.insert(
            entry.head_ref_name.clone(),
            PrEntry {
                repo: repo_label.unwrap_or("").to_string(),
                number: entry.number,
                state: entry.state.to_lowercase(),
                url: entry.url,
                updated_at: entry.updated_at,
                ci_status: summarize_checks(&entry.status_check_rollup),
                ci_checks: map_checks(&entry.status_check_rollup),
                comments,
            },
        );
    }
    Ok(prs)
}

// ── I/O ─────────────────────────────────────────────────────────────────────

/// Run `gh <args>` in `cwd` with a hard timeout, returning stdout.
fn run_gh(args: &[&str], cwd: &str, timeout: Duration) -> Result<String, String> {
    let mut child = Command::new("gh")
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("gh spawn failed: {e}"))?;

    let (tx_out, rx_out) = std::sync::mpsc::channel();
    if let Some(mut out) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = out.read_to_string(&mut s);
            let _ = tx_out.send(s);
        });
    }
    let mut stderr_pipe = child.stderr.take();

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = rx_out.recv().unwrap_or_default();
                if status.success() {
                    return Ok(stdout);
                }
                let mut stderr = String::new();
                if let Some(pipe) = stderr_pipe.as_mut() {
                    let _ = pipe.read_to_string(&mut stderr);
                }
                return Err(format!("gh exited {}: {}", status.code().unwrap_or(-1), stderr.trim()));
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    return Err("gh timed out".to_string());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Fetch open PRs for a repo (current repo when `repo_slug` is `None`).
fn fetch_all_prs(
    repo_slug: Option<&str>,
    repo_label: Option<&str>,
    cwd: &str,
) -> Result<HashMap<String, PrEntry>, String> {
    let limit = PR_FETCH_LIMIT.to_string();
    let mut args = vec![
        "pr", "list", "--state", "open", "--json",
        "number,headRefName,state,updatedAt,statusCheckRollup,url,comments",
        "--limit", &limit,
    ];
    if let Some(slug) = repo_slug {
        args.push("--repo");
        args.push(slug);
    }
    let json = run_gh(&args, cwd, GH_TIMEOUT)?;
    parse_pr_response(&json, repo_label)
}

/// Branch → git dir for every live managed worktree (used to place `prs.json`).
fn worktree_git_dirs(git: &GitGateway, project_root: &str) -> HashMap<String, String> {
    let root = std::fs::canonicalize(project_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| project_root.to_string());
    let mut map = HashMap::new();
    for entry in git.list_live_worktrees(&root) {
        if entry.bare {
            continue;
        }
        let Some(branch) = entry.branch.clone() else {
            continue;
        };
        if let Ok(git_dir) = git.resolve_worktree_git_dir(&entry.path) {
            map.insert(branch, git_dir);
        }
    }
    map
}

/// Fetch PRs for the project + linked repos and write `prs.json` for each
/// worktree with matching open PRs. Blocking.
pub fn sync_pr_status(git: &GitGateway, project_root: &str, linked_repos: &[LinkedRepoConfig]) {
    let mut branch_prs: HashMap<String, Vec<PrEntry>> = HashMap::new();

    let mut collect = |result: Result<HashMap<String, PrEntry>, String>| match result {
        Ok(map) => {
            for (branch, entry) in map {
                branch_prs.entry(branch).or_default().push(entry);
            }
        }
        Err(err) => tracing::error!("[pr] {err}"),
    };

    collect(fetch_all_prs(None, None, project_root));
    for lr in linked_repos {
        collect(fetch_all_prs(Some(&lr.repo), Some(&lr.alias), project_root));
    }

    let git_dirs = worktree_git_dirs(git, project_root);
    let mut seen = std::collections::HashSet::new();
    for (branch, entries) in &branch_prs {
        let Some(git_dir) = git_dirs.get(branch) else {
            continue;
        };
        if !seen.insert(git_dir.clone()) {
            continue;
        }
        if let Err(e) = write_worktree_prs(git_dir, entries) {
            tracing::warn!("[pr] failed to write prs.json for {branch}: {e}");
        }
    }
}

/// Fetch failed CI logs for a run via `gh run view <id> --log-failed`.
pub fn fetch_ci_logs(run_id: i64, cwd: &str) -> Result<String, String> {
    run_gh(&["run", "view", &run_id.to_string(), "--log-failed"], cwd, GH_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_run_id_from_actions_url() {
        assert_eq!(parse_run_id(Some("https://github.com/o/r/actions/runs/12345/job/9")), Some(12345));
        assert_eq!(parse_run_id(Some("https://vercel.com/x")), None);
        assert_eq!(parse_run_id(None), None);
    }

    #[test]
    fn summarize_and_map_mixed_checks() {
        let json = r#"[{
            "number": 7, "headRefName": "feature", "state": "OPEN", "updatedAt": "t",
            "url": "https://gh/pr/7",
            "statusCheckRollup": [
                {"__typename":"CheckRun","status":"COMPLETED","conclusion":"SUCCESS","name":"build","detailsUrl":"https://github.com/o/r/actions/runs/42"},
                {"__typename":"StatusContext","state":"SUCCESS","context":"vercel","targetUrl":"https://vercel.com/x"}
            ],
            "comments": [{"author":{"login":"bob"},"body":"lgtm","createdAt":"t2"}]
        }]"#;
        let prs = parse_pr_response(json, Some("svc")).unwrap();
        let pr = prs.get("feature").unwrap();
        assert_eq!(pr.repo, "svc");
        assert_eq!(pr.number, 7);
        assert_eq!(pr.state, "open");
        assert_eq!(pr.ci_status, "success");
        assert_eq!(pr.ci_checks.len(), 2);
        assert_eq!(pr.ci_checks[0].run_id, Some(42));
        assert_eq!(pr.comments.len(), 1);
        assert_eq!(pr.comments[0].author, "bob");
    }

    #[test]
    fn pending_when_a_check_is_in_progress() {
        let json = r#"[{
            "number": 1, "headRefName": "b", "state": "OPEN", "updatedAt": "t", "url": "u",
            "statusCheckRollup": [{"__typename":"CheckRun","status":"IN_PROGRESS","conclusion":null,"name":"x","detailsUrl":null}],
            "comments": []
        }]"#;
        let prs = parse_pr_response(json, None).unwrap();
        assert_eq!(prs.get("b").unwrap().ci_status, "pending");
    }
}
