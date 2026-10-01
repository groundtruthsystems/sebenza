//! Inbox subcommands: `ls`, `show`, `new`, `edit`, `link`, `unlink`, `drop`, `rm`,
//! `convert`, `job`, `priority`, `comment`, `comments`, `requests`.
//!
//! The inbox is global — drafts exist before they belong to any project — so
//! these talk to the hub routes rather than a project-prefixed base.
//!
//! Mutating calls carry the control token from
//! `~/.config/sebenza/control-token`, which the server requires. The CLI sends
//! no `Origin`, which is expected and is exactly why the token is not optional.

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use crate::http::Http;

enum InboxCommand {
    Ls {
        search: Option<String>,
        all: bool,
    },
    Show(String),
    New(String),
    Edit(String),
    Link {
        id: String,
        path: String,
    },
    Unlink(String),
    Drop(String),
    Rm {
        id: String,
        yes: bool,
    },
    Convert {
        id: String,
        specs: Vec<String>,
        base: Option<String>,
        watch: bool,
    },
    Job(String),
    /// `None` clears the override.
    Priority {
        id: String,
        priority: Option<String>,
    },
    Comment {
        id: String,
        body: String,
        /// `project:branch`; absent posts to the overall thread.
        worktree: Option<(String, String)>,
    },
    Comments(String),
    Requests(String),
    /// Confirm a request's proposal, or (`--body`) an edited or authored
    /// resolution, and deliver it.
    Confirm {
        id: String,
        request_id: String,
        body: Option<String>,
    },
    Reject {
        id: String,
        request_id: String,
        reason: String,
    },
    Redeliver {
        id: String,
        request_id: String,
    },
    RetryTriage {
        id: String,
        request_id: String,
    },
    AgentJob {
        id: String,
        job_id: String,
    },
    Redact {
        id: String,
        event_id: String,
    },
}

fn usage() -> String {
    [
        "Usage:",
        "  sebenza-cli inbox ls [--search TEXT] [--all]   List drafts (--all includes dropped)",
        "  sebenza-cli inbox show <id>                    Print a draft",
        "  sebenza-cli inbox new <title>                  Create an empty draft",
        "  sebenza-cli inbox edit <id>                    Open the draft in $EDITOR and save",
        "  sebenza-cli inbox link <id> <path>             Link the draft to a project",
        "  sebenza-cli inbox unlink <id>                  Remove the project link",
        "  sebenza-cli inbox drop <id>                    Mark a draft dropped",
        "  sebenza-cli inbox rm <id> [--yes]              Delete a draft",
        "  sebenza-cli inbox convert <id> <target>... [--base B]  Turn a draft into worktrees",
        "  sebenza-cli inbox job <job-id>                 Show a conversion's progress",
        "  sebenza-cli inbox priority <id> <P0-P3|clear>  Set or clear the priority override",
        "  sebenza-cli inbox comment <id> [--worktree project:branch] <text>",
        "                                                 Comment overall or on one worktree",
        "  sebenza-cli inbox comments <id>                Show the overall and worktree threads",
        "  sebenza-cli inbox requests <id>                List requests worktree agents raised",
        "",
        "A convert target is project:branch:prompt, for example:",
        "  sebenza-cli inbox convert 01ARZ... ~/code/acme:fix-scorer:'rewrite the scorer'",
        "",
        "Each worktree forks from the project's default branch unless told otherwise.",
        "--base sets the source branch for every target; branch@source overrides it for",
        "one, which is what you want when targets span projects:",
        "  sebenza-cli inbox convert 01ARZ... --base develop \\",
        "    ~/code/acme:fix-scorer:'rewrite it' ~/code/beta:hotfix@main:'patch it'",
        "",
        "Pass --watch to poll until the fan-out finishes.",
        "",
        "Drafts list by priority (P0 first), then newest. A priority you set is an",
        "override the system agent will not change; `priority <id> clear` hands it back.",
        "",
        "Drafts are markdown files under ~/.ai/sebenza/inbox/, global across every",
        "project. A draft that has been converted into worktrees needs --yes to",
        "delete, because its conversion history is the only record of the prompts",
        "that were sent.",
        "",
        "Examples:",
        "  sebenza-cli inbox new 'Rework the claims scorer'",
        "  sebenza-cli inbox ls --search scorer",
        "  sebenza-cli inbox link 01ARZ3NDEKTSV4RRFFQ69G5FAV ~/code/claims",
    ]
    .join("\n")
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn opt(args: &[String], name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).cloned()
}

fn positional(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip_next = false;
    for (i, a) in args.iter().enumerate().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if a == "--search" || a == "--base" || a == "--worktree" {
            skip_next = true;
            continue;
        }
        if a.starts_with("--") {
            continue;
        }
        let _ = i;
        out.push(a.clone());
    }
    out
}

fn parse(args: &[String]) -> Result<Option<InboxCommand>> {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        return Ok(None);
    }
    let pos = positional(args);
    let need = |n: usize, what: &str| -> Result<String> {
        pos.get(n).cloned().ok_or_else(|| anyhow!("Missing {what}"))
    };
    match args[0].as_str() {
        "ls" | "list" => Ok(Some(InboxCommand::Ls {
            search: opt(args, "--search"),
            all: flag(args, "--all"),
        })),
        "show" | "cat" => Ok(Some(InboxCommand::Show(need(0, "draft id")?))),
        "new" => Ok(Some(InboxCommand::New(need(0, "title")?))),
        "edit" => Ok(Some(InboxCommand::Edit(need(0, "draft id")?))),
        "link" => Ok(Some(InboxCommand::Link {
            id: need(0, "draft id")?,
            path: need(1, "project path")?,
        })),
        "unlink" => Ok(Some(InboxCommand::Unlink(need(0, "draft id")?))),
        "drop" => Ok(Some(InboxCommand::Drop(need(0, "draft id")?))),
        "convert" => {
            let id = need(0, "draft id")?;
            let specs: Vec<String> = pos.into_iter().skip(1).collect();
            if specs.is_empty() {
                return Err(anyhow!("Missing at least one project:branch:prompt target"));
            }
            Ok(Some(InboxCommand::Convert {
                id,
                specs,
                base: opt(args, "--base"),
                watch: flag(args, "--watch"),
            }))
        }
        "job" => Ok(Some(InboxCommand::Job(need(0, "job id")?))),
        "rm" | "remove" | "delete" => Ok(Some(InboxCommand::Rm {
            id: need(0, "draft id")?,
            yes: flag(args, "--yes") || flag(args, "-y"),
        })),
        "priority" => {
            let id = need(0, "draft id")?;
            let level = need(1, "priority (P0-P3, or clear)")?;
            Ok(Some(InboxCommand::Priority {
                id,
                priority: parse_priority(&level)?,
            }))
        }
        "comment" => {
            let id = need(0, "draft id")?;
            let body = pos[1..].join(" ");
            if body.trim().is_empty() {
                return Err(anyhow!("Missing comment text"));
            }
            let worktree = match opt(args, "--worktree") {
                Some(spec) => Some(parse_worktree(&spec)?),
                None => None,
            };
            Ok(Some(InboxCommand::Comment { id, body, worktree }))
        }
        "comments" => Ok(Some(InboxCommand::Comments(need(0, "draft id")?))),
        "requests" => Ok(Some(InboxCommand::Requests(need(0, "draft id")?))),
        other => Err(anyhow!("Unknown inbox command: {other}")),
    }
}

/// `P0`-`P3` (any case) sets an override; `clear` or `none` removes it.
fn parse_priority(raw: &str) -> Result<Option<String>> {
    let level = raw.trim().to_ascii_uppercase();
    match level.as_str() {
        "CLEAR" | "NONE" => Ok(None),
        "P0" | "P1" | "P2" | "P3" => Ok(Some(level)),
        _ => Err(anyhow!(
            "priority must be P0, P1, P2, P3 or clear, not {raw:?}"
        )),
    }
}

/// `project:branch`. Split at the last colon: a branch cannot contain one,
/// but a path might.
fn parse_worktree(spec: &str) -> Result<(String, String)> {
    match spec.rsplit_once(':') {
        Some((project, branch)) if !project.trim().is_empty() && !branch.trim().is_empty() => {
            Ok((project.trim().to_string(), branch.trim().to_string()))
        }
        _ => Err(anyhow!("--worktree {spec:?} is not project:branch")),
    }
}

/// The hash a confirm must quote: of the proposal the operator is
/// confirming (edited or not), else of the resolution they authored.
fn confirm_hash(request: &Value, body: Option<&str>) -> Result<String> {
    let _ = (request, body);
    Err(anyhow!("not implemented"))
}

fn print_comment_rows(rows: &[Value]) {
    if rows.is_empty() {
        println!("  (no comments)");
    }
    for c in rows {
        let ts = c.get("ts").and_then(Value::as_str).unwrap_or("");
        let author = c.get("author").and_then(Value::as_str).unwrap_or("");
        let kind = c.get("kind").and_then(Value::as_str).unwrap_or("");
        let body = c.get("body").and_then(Value::as_str).unwrap_or("");
        let flag = if c
            .get("warnings")
            .and_then(Value::as_array)
            .is_some_and(|w| !w.is_empty())
        {
            "  [possible secret]"
        } else {
            ""
        };
        println!("  {ts}  {author:<14} {kind:<10}{flag}");
        for line in body.lines() {
            println!("      {line}");
        }
    }
}

fn print_comments(groups: &Value) {
    println!("Overall");
    let rows = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
    print_comment_rows(&rows(groups.get("overall")));
    for g in rows(groups.get("worktrees")) {
        println!();
        println!(
            "{}:{}",
            g.get("project").and_then(Value::as_str).unwrap_or(""),
            g.get("branch").and_then(Value::as_str).unwrap_or("")
        );
        print_comment_rows(&rows(g.get("comments")));
    }
}

fn print_requests(body: &Value) {
    let requests = body
        .get("requests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if requests.is_empty() {
        println!("No requests.");
        return;
    }
    for r in requests {
        let id = r.get("requestId").and_then(Value::as_str).unwrap_or("");
        let status = r.get("status").and_then(Value::as_str).unwrap_or("");
        let flagged = if r.get("flagged").and_then(Value::as_bool).unwrap_or(false) {
            " (flagged)"
        } else {
            ""
        };
        let wt = r.get("worktree").cloned().unwrap_or(Value::Null);
        let branch = wt.get("branch").and_then(Value::as_str).unwrap_or("");
        let title = r.get("title").and_then(Value::as_str).unwrap_or("");
        println!("{id}  {status:<15}{flagged} [{branch}] {title}");
    }
}

/// Expand a leading `~` so `inbox link <id> ~/code/x` stores an absolute path,
/// which is what the server requires.
fn expand_home(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var("HOME") {
            Ok(home) => format!("{home}/{rest}"),
            Err(_) => path.to_string(),
        },
        None => match std::fs::canonicalize(path) {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => path.to_string(),
        },
    }
}

fn status_of(draft: &Value) -> &str {
    draft.get("status").and_then(Value::as_str).unwrap_or("")
}

fn print_list(body: &Value) {
    let drafts = body
        .get("drafts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if drafts.is_empty() {
        println!("No drafts. Create one with: sebenza-cli inbox new '<title>'");
        return;
    }
    for d in drafts {
        let id = d.get("id").and_then(Value::as_str).unwrap_or("");
        let raw = d.get("isRaw").and_then(Value::as_bool).unwrap_or(false);
        let title = if raw {
            "(unparseable)"
        } else {
            d.get("title").and_then(Value::as_str).unwrap_or("")
        };
        let project = d
            .get("project")
            .and_then(|p| {
                let path = p.get("path").and_then(Value::as_str)?;
                let resolved = p.get("resolved").and_then(Value::as_bool).unwrap_or(false);
                Some(if resolved {
                    format!(
                        "  [{}]",
                        p.get("name").and_then(Value::as_str).unwrap_or(path)
                    )
                } else {
                    format!("  [unresolved: {path}]")
                })
            })
            .unwrap_or_default();
        let priority = d.get("priority").and_then(Value::as_str).unwrap_or("P2");
        let pinned = if d.get("prioritySource").and_then(Value::as_str) == Some("operator") {
            "*"
        } else {
            " "
        };
        println!(
            "{id}  {priority}{pinned} {:<9} {title}{project}",
            status_of(&d)
        );
    }
}

/// Parse `project:branch:prompt`.
///
/// Split from the left twice only, so a prompt may contain colons — which
/// prompts routinely do.
fn parse_target(spec: &str, default_base: Option<&str>) -> Result<Value> {
    let (project, rest) = spec
        .split_once(':')
        .ok_or_else(|| anyhow!("target {spec:?} is not project:branch:prompt"))?;
    let (branch_spec, prompt) = rest
        .split_once(':')
        .ok_or_else(|| anyhow!("target {spec:?} is missing a prompt"))?;

    // `branch@source` overrides --base for this target, which is what you want
    // when targets span projects whose default branches differ.
    let (branch, base) = match branch_spec.split_once('@') {
        Some((b, s)) => (b, Some(s.trim().to_string())),
        None => (branch_spec, default_base.map(str::to_string)),
    };

    if project.trim().is_empty() || branch.trim().is_empty() || prompt.trim().is_empty() {
        return Err(anyhow!("target {spec:?} has an empty field"));
    }
    if base.as_deref().map(str::trim) == Some("") {
        return Err(anyhow!("target {spec:?} has an empty source branch"));
    }

    let mut out = json!({
        "projectPath": expand_home(project.trim()),
        "branch": branch.trim(),
        "prompt": prompt.trim(),
    });
    if let Some(base) = base {
        out["baseBranch"] = Value::String(base);
    }
    Ok(out)
}

fn print_job(job: &Value) {
    let outcomes = job
        .get("outcomes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total = job.get("total").and_then(Value::as_u64).unwrap_or(0);
    for o in &outcomes {
        let branch = o.get("branch").and_then(Value::as_str).unwrap_or("");
        let outcome = o.get("outcome").and_then(Value::as_str).unwrap_or("");
        let detail = o
            .get("worktreePath")
            .or_else(|| o.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("");
        println!("  {outcome:<8} {branch:<28} {detail}");
    }
    let created = outcomes
        .iter()
        .filter(|o| o.get("outcome").and_then(Value::as_str) == Some("created"))
        .count();
    if job
        .get("finished")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        println!("{created}/{total} created.");
        if let Some(err) = job.get("error").and_then(Value::as_str) {
            eprintln!("job failed: {err}");
        }
    } else {
        println!("{}/{total} done so far.", outcomes.len());
    }
}

/// Run the draft's body through `$EDITOR`, then save it with the hash it was
/// read at — so a concurrent change is a conflict rather than a silent
/// overwrite.
async fn edit_draft(http: &Http, id: &str) -> Result<()> {
    let draft = http.inbox_get(id).await?;
    if draft.get("raw").map(|r| !r.is_null()).unwrap_or(false) {
        return Err(anyhow!(
            "draft {id} does not parse; fix it in ~/.ai/sebenza/inbox/{id}.md"
        ));
    }
    let body = draft.get("body").and_then(Value::as_str).unwrap_or("");
    let hash = draft.get("bodyHash").and_then(Value::as_str).unwrap_or("");

    let tmp = std::env::temp_dir().join(format!("sebenza-inbox-{id}.md"));
    std::fs::write(&tmp, body)?;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = std::process::Command::new(&editor).arg(&tmp).status()?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(anyhow!("{editor} exited non-zero; draft not saved"));
    }
    let edited = std::fs::read_to_string(&tmp)?;
    let _ = std::fs::remove_file(&tmp);
    if edited == body {
        println!("No change.");
        return Ok(());
    }
    http.inbox_save_body(id, hash, &edited).await?;
    println!("Saved {id}.");
    Ok(())
}

pub async fn run(args: &[String], port: u16) -> i32 {
    let command = match parse(args) {
        Ok(Some(c)) => c,
        Ok(None) => {
            println!("{}", usage());
            return 0;
        }
        Err(e) => {
            eprintln!("{e}\n\n{}", usage());
            return 1;
        }
    };
    let http = Http::new(port);

    let result: Result<()> = async {
        match command {
            InboxCommand::Ls { search, all } => {
                print_list(&http.inbox_list(search.as_deref(), all).await?);
            }
            InboxCommand::Show(id) => {
                let d = http.inbox_get(&id).await?;
                if let Some(raw) = d.get("raw").filter(|r| !r.is_null()) {
                    eprintln!(
                        "draft does not parse: {}",
                        raw.get("error").and_then(Value::as_str).unwrap_or("")
                    );
                    println!("{}", raw.get("text").and_then(Value::as_str).unwrap_or(""));
                } else {
                    println!("# {}", d.get("title").and_then(Value::as_str).unwrap_or(""));
                    println!();
                    println!("{}", d.get("body").and_then(Value::as_str).unwrap_or(""));
                }
            }
            InboxCommand::New(title) => {
                let d = http.inbox_create(&title).await?;
                println!("{}", d.get("id").and_then(Value::as_str).unwrap_or(""));
            }
            InboxCommand::Edit(id) => edit_draft(&http, &id).await?,
            InboxCommand::Link { id, path } => {
                http.inbox_patch(&id, json!({ "projectPath": expand_home(&path) }))
                    .await?;
                println!("Linked {id}.");
            }
            InboxCommand::Unlink(id) => {
                http.inbox_patch(&id, json!({ "projectPath": null }))
                    .await?;
                println!("Unlinked {id}.");
            }
            InboxCommand::Drop(id) => {
                http.inbox_patch(&id, json!({ "status": "Dropped" }))
                    .await?;
                println!("Dropped {id}.");
            }
            InboxCommand::Convert {
                id,
                specs,
                base,
                watch,
            } => {
                let targets: Result<Vec<Value>> = specs
                    .iter()
                    .map(|s| parse_target(s, base.as_deref()))
                    .collect();
                let started = http.inbox_convert(&id, Value::Array(targets?)).await?;
                let job_id = started
                    .get("jobId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("server did not return a job id"))?
                    .to_string();
                println!("job {job_id}");
                if !watch {
                    println!("Follow it with: sebenza-cli inbox job {job_id}");
                } else {
                    // Poll rather than hold a socket: the CLI is an HTTP client
                    // and the snapshot route is authoritative anyway.
                    loop {
                        let job = http.inbox_job(&job_id).await?;
                        if job
                            .get("finished")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        {
                            print_job(&job);
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
                    }
                }
            }
            InboxCommand::Job(job_id) => print_job(&http.inbox_job(&job_id).await?),
            InboxCommand::Priority { id, priority } => {
                let d = http
                    .inbox_set_priority(&id, json!({ "priority": priority }))
                    .await?;
                let level = d.get("priority").and_then(Value::as_str).unwrap_or("");
                match priority {
                    Some(_) => println!("{id} is {level} (operator override)."),
                    None => println!(
                        "Cleared the override on {id}; it stays {level} until the agent re-ranks it."
                    ),
                }
            }
            InboxCommand::Comment { id, body, worktree } => {
                let mut payload = json!({ "body": body });
                if let Some((project, branch)) = worktree {
                    payload["worktree"] = json!({
                        "project": expand_home(&project),
                        "branch": branch,
                    });
                }
                http.inbox_post_comment(&id, payload).await?;
                println!("Commented on {id}.");
            }
            InboxCommand::Comments(id) => print_comments(&http.inbox_comments(&id).await?),
            InboxCommand::Requests(id) => print_requests(&http.inbox_requests(&id).await?),
            InboxCommand::Confirm { .. }
            | InboxCommand::Reject { .. }
            | InboxCommand::Redeliver { .. }
            | InboxCommand::RetryTriage { .. }
            | InboxCommand::AgentJob { .. }
            | InboxCommand::Redact { .. } => return Err(anyhow!("not implemented")),
            InboxCommand::Rm { id, yes } => {
                http.inbox_delete(&id, yes).await?;
                println!("Deleted {id}.");
            }
        }
        Ok(())
    }
    .await;

    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn ls_accepts_search_and_all() {
        let cmd = parse(&a(&["ls", "--search", "scorer", "--all"]))
            .expect("parse")
            .expect("command");
        match cmd {
            InboxCommand::Ls { search, all } => {
                assert_eq!(search.as_deref(), Some("scorer"));
                assert!(all);
            }
            _ => panic!("expected Ls"),
        }
    }

    #[test]
    fn the_search_value_is_not_read_as_a_positional() {
        // `inbox ls --search show` must not be parsed as the `show` argument.
        let cmd = parse(&a(&["ls", "--search", "show"]))
            .expect("parse")
            .expect("command");
        match cmd {
            InboxCommand::Ls { search, .. } => assert_eq!(search.as_deref(), Some("show")),
            _ => panic!("expected Ls"),
        }
    }

    #[test]
    fn rm_requires_yes_to_be_explicit() {
        let cmd = parse(&a(&["rm", "01ARZ3NDEKTSV4RRFFQ69G5FAV"]))
            .expect("parse")
            .expect("command");
        match cmd {
            InboxCommand::Rm { yes, .. } => assert!(!yes),
            _ => panic!("expected Rm"),
        }
        let cmd = parse(&a(&["rm", "01ARZ3NDEKTSV4RRFFQ69G5FAV", "--yes"]))
            .expect("parse")
            .expect("command");
        match cmd {
            InboxCommand::Rm { yes, .. } => assert!(yes),
            _ => panic!("expected Rm"),
        }
    }

    #[test]
    fn link_needs_both_arguments() {
        assert!(parse(&a(&["link", "01ARZ3NDEKTSV4RRFFQ69G5FAV"])).is_err());
        let cmd = parse(&a(&["link", "01ARZ3NDEKTSV4RRFFQ69G5FAV", "/code/x"]))
            .expect("parse")
            .expect("command");
        match cmd {
            InboxCommand::Link { path, .. } => assert_eq!(path, "/code/x"),
            _ => panic!("expected Link"),
        }
    }

    #[test]
    fn unknown_subcommand_is_an_error_not_a_silent_help() {
        assert!(parse(&a(&["frobnicate"])).is_err());
    }

    #[test]
    fn help_and_empty_render_usage() {
        assert!(parse(&[]).expect("parse").is_none());
        assert!(parse(&a(&["--help"])).expect("parse").is_none());
    }

    #[test]
    fn expand_home_makes_a_tilde_path_absolute() {
        unsafe { std::env::set_var("HOME", "/home/tester") };
        assert_eq!(expand_home("~/code/x"), "/home/tester/code/x");
        assert_eq!(expand_home("/already/absolute"), "/already/absolute");
    }

    #[test]
    fn a_target_spec_splits_into_project_branch_and_prompt() {
        let v = parse_target("/code/acme:fix-scorer:rewrite it", None).expect("parse");
        assert_eq!(v["projectPath"], "/code/acme");
        assert_eq!(v["branch"], "fix-scorer");
        assert_eq!(v["prompt"], "rewrite it");
    }

    #[test]
    fn a_prompt_may_contain_colons() {
        // Prompts routinely do: "fix: the thing". Splitting from the left
        // twice is what keeps that working.
        let v = parse_target("/code/acme:b:fix: the thing, then: ship", None).expect("parse");
        assert_eq!(v["branch"], "b");
        assert_eq!(v["prompt"], "fix: the thing, then: ship");
    }

    #[test]
    fn a_malformed_target_is_refused() {
        assert!(parse_target("just-a-word", None).is_err());
        assert!(parse_target("/code/acme:branch-only", None).is_err());
        assert!(parse_target("/code/acme::no branch", None).is_err());
        assert!(parse_target("/code/acme:b:   ", None).is_err());
    }

    #[test]
    fn convert_needs_at_least_one_target() {
        assert!(parse(&a(&["convert", "01ARZ3NDEKTSV4RRFFQ69G5FAV"])).is_err());
        let cmd = parse(&a(&[
            "convert",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "/code/acme:b:do it",
        ]))
        .expect("parse")
        .expect("command");
        match cmd {
            InboxCommand::Convert { specs, watch, .. } => {
                assert_eq!(specs.len(), 1);
                assert!(!watch);
            }
            _ => panic!("expected Convert"),
        }
    }

    #[test]
    fn no_source_branch_means_the_project_default() {
        let v = parse_target("/code/acme:b:go", None).expect("parse");
        assert!(
            v.get("baseBranch").is_none(),
            "an absent base must not be sent as empty"
        );
    }

    #[test]
    fn the_base_flag_applies_to_every_target() {
        let v = parse_target("/code/acme:b:go", Some("develop")).expect("parse");
        assert_eq!(v["baseBranch"], "develop");
    }

    #[test]
    fn branch_at_source_overrides_the_base_flag() {
        // Targets spanning projects rarely share a default branch.
        let v = parse_target("/code/beta:hotfix@main:patch it", Some("develop")).expect("parse");
        assert_eq!(v["branch"], "hotfix");
        assert_eq!(v["baseBranch"], "main");
        assert_eq!(v["prompt"], "patch it");
    }

    #[test]
    fn an_empty_source_branch_is_refused() {
        assert!(parse_target("/code/acme:b@:go", None).is_err());
    }

    #[test]
    fn the_base_value_is_not_read_as_a_positional() {
        let cmd = parse(&a(&[
            "convert",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--base",
            "develop",
            "/code/acme:b:go",
        ]))
        .expect("parse")
        .expect("command");
        match cmd {
            InboxCommand::Convert { specs, base, .. } => {
                assert_eq!(base.as_deref(), Some("develop"));
                assert_eq!(specs, vec!["/code/acme:b:go".to_string()]);
            }
            _ => panic!("expected Convert"),
        }
    }

    #[test]
    fn priority_takes_a_level_or_clear() {
        let cmd = parse(&a(&["priority", "01ARZ3NDEKTSV4RRFFQ69G5FAV", "p0"]))
            .expect("parse")
            .expect("command");
        match cmd {
            InboxCommand::Priority { priority, .. } => assert_eq!(priority.as_deref(), Some("P0")),
            _ => panic!("expected Priority"),
        }
        let cmd = parse(&a(&["priority", "01ARZ3NDEKTSV4RRFFQ69G5FAV", "clear"]))
            .expect("parse")
            .expect("command");
        match cmd {
            InboxCommand::Priority { priority, .. } => assert_eq!(priority, None),
            _ => panic!("expected Priority"),
        }
        assert!(parse(&a(&["priority", "01ARZ3NDEKTSV4RRFFQ69G5FAV", "P7"])).is_err());
        assert!(parse(&a(&["priority", "01ARZ3NDEKTSV4RRFFQ69G5FAV"])).is_err());
    }

    #[test]
    fn comment_posts_overall_or_to_a_worktree() {
        let cmd = parse(&a(&["comment", "01ARZ3NDEKTSV4RRFFQ69G5FAV", "looks good"]))
            .expect("parse")
            .expect("command");
        match cmd {
            InboxCommand::Comment { body, worktree, .. } => {
                assert_eq!(body, "looks good");
                assert_eq!(worktree, None);
            }
            _ => panic!("expected Comment"),
        }
        let cmd = parse(&a(&[
            "comment",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--worktree",
            "/code/acme-demo:feat/x",
            "try sqlite",
        ]))
        .expect("parse")
        .expect("command");
        match cmd {
            InboxCommand::Comment { body, worktree, .. } => {
                assert_eq!(body, "try sqlite", "the --worktree value is not the body");
                assert_eq!(
                    worktree,
                    Some(("/code/acme-demo".to_string(), "feat/x".to_string()))
                );
            }
            _ => panic!("expected Comment"),
        }
        assert!(parse(&a(&["comment", "01ARZ3NDEKTSV4RRFFQ69G5FAV"])).is_err());
        assert!(
            parse(&a(&[
                "comment",
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "--worktree",
                "no-branch",
                "x"
            ]))
            .is_err()
        );
    }

    #[test]
    fn comments_and_requests_take_an_id() {
        assert!(matches!(
            parse(&a(&["comments", "01ARZ3NDEKTSV4RRFFQ69G5FAV"])).unwrap(),
            Some(InboxCommand::Comments(_))
        ));
        assert!(matches!(
            parse(&a(&["requests", "01ARZ3NDEKTSV4RRFFQ69G5FAV"])).unwrap(),
            Some(InboxCommand::Requests(_))
        ));
        assert!(parse(&a(&["requests"])).is_err());
    }

    // FR-33: the CLI reaches every new route.
    #[test]
    fn usage_lists_the_collaboration_commands() {
        let u = usage();
        for cmd in [
            "inbox priority",
            "inbox comment ",
            "inbox comments",
            "inbox requests",
        ] {
            assert!(u.contains(cmd), "usage is missing {cmd}");
        }
    }

    #[test]
    fn confirm_takes_an_optional_body() {
        match parse(&a(&["confirm", "D1", "R1"])).unwrap().unwrap() {
            InboxCommand::Confirm {
                id,
                request_id,
                body,
            } => {
                assert_eq!((id.as_str(), request_id.as_str()), ("D1", "R1"));
                assert!(body.is_none());
            }
            _ => panic!("expected Confirm"),
        }
        match parse(&a(&["confirm", "D1", "R1", "--body", "use v2"]))
            .unwrap()
            .unwrap()
        {
            InboxCommand::Confirm { body, .. } => assert_eq!(body.as_deref(), Some("use v2")),
            _ => panic!("expected Confirm"),
        }
        assert!(parse(&a(&["confirm", "D1"])).is_err());
    }

    #[test]
    fn reject_needs_a_reason() {
        assert!(parse(&a(&["reject", "D1", "R1"])).is_err());
        match parse(&a(&["reject", "D1", "R1", "wrong", "loader"]))
            .unwrap()
            .unwrap()
        {
            InboxCommand::Reject { reason, .. } => assert_eq!(reason, "wrong loader"),
            _ => panic!("expected Reject"),
        }
    }

    #[test]
    fn redeliver_retry_job_and_redact_parse() {
        assert!(matches!(
            parse(&a(&["redeliver", "D1", "R1"])).unwrap(),
            Some(InboxCommand::Redeliver { .. })
        ));
        assert!(matches!(
            parse(&a(&["retry-triage", "D1", "R1"])).unwrap(),
            Some(InboxCommand::RetryTriage { .. })
        ));
        assert!(matches!(
            parse(&a(&["agent-job", "D1", "J1"])).unwrap(),
            Some(InboxCommand::AgentJob { .. })
        ));
        assert!(matches!(
            parse(&a(&["redact", "D1", "E1"])).unwrap(),
            Some(InboxCommand::Redact { .. })
        ));
        assert!(parse(&a(&["redact", "D1"])).is_err());
    }

    // TS-32: agentctl has no confirm; the operator CLI's usage names it.
    #[test]
    fn usage_lists_the_decision_commands() {
        let u = usage();
        for cmd in [
            "confirm",
            "reject",
            "redeliver",
            "retry-triage",
            "agent-job",
            "redact",
        ] {
            assert!(u.contains(&format!("inbox {cmd} ")), "usage lacks {cmd}");
        }
    }

    #[test]
    fn the_confirm_hash_is_of_the_text_shown() {
        let proposal = json!({"proposal": "use the loader", "proposalHash": "server"});
        // With a proposal the hash is of the proposal, even when editing.
        assert_eq!(
            confirm_hash(&proposal, Some("edited")).unwrap(),
            common::domain::inbox_events::content_hash("use the loader")
        );
        // Without one, of the authored body.
        let open = json!({"proposal": null});
        assert_eq!(
            confirm_hash(&open, Some("authored")).unwrap(),
            common::domain::inbox_events::content_hash("authored")
        );
        assert!(confirm_hash(&open, None).is_err());
    }
}
