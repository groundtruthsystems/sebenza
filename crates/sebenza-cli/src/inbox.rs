//! Inbox subcommands: `ls`, `show`, `new`, `edit`, `link`, `unlink`, `drop`, `rm`.
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
        watch: bool,
    },
    Job(String),
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
        "  sebenza-cli inbox convert <id> <target>...      Turn a draft into worktrees",
        "  sebenza-cli inbox job <job-id>                 Show a conversion's progress",
        "",
        "A convert target is project:branch:prompt, for example:",
        "  sebenza-cli inbox convert 01ARZ... ~/code/acme:fix-scorer:'rewrite the scorer'",
        "Pass --watch to poll until the fan-out finishes.",
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
        if a == "--search" {
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
                watch: flag(args, "--watch"),
            }))
        }
        "job" => Ok(Some(InboxCommand::Job(need(0, "job id")?))),
        "rm" | "remove" | "delete" => Ok(Some(InboxCommand::Rm {
            id: need(0, "draft id")?,
            yes: flag(args, "--yes") || flag(args, "-y"),
        })),
        other => Err(anyhow!("Unknown inbox command: {other}")),
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
        println!("{id}  {:<9} {title}{project}", status_of(&d));
    }
}

/// Parse `project:branch:prompt`.
///
/// Split from the left twice only, so a prompt may contain colons — which
/// prompts routinely do.
fn parse_target(spec: &str) -> Result<Value> {
    let (project, rest) = spec
        .split_once(':')
        .ok_or_else(|| anyhow!("target {spec:?} is not project:branch:prompt"))?;
    let (branch, prompt) = rest
        .split_once(':')
        .ok_or_else(|| anyhow!("target {spec:?} is missing a prompt"))?;
    if project.trim().is_empty() || branch.trim().is_empty() || prompt.trim().is_empty() {
        return Err(anyhow!("target {spec:?} has an empty field"));
    }
    Ok(json!({
        "projectPath": expand_home(project.trim()),
        "branch": branch.trim(),
        "prompt": prompt.trim(),
    }))
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
            InboxCommand::Convert { id, specs, watch } => {
                let targets: Result<Vec<Value>> = specs.iter().map(|s| parse_target(s)).collect();
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
        let v = parse_target("/code/acme:fix-scorer:rewrite it").expect("parse");
        assert_eq!(v["projectPath"], "/code/acme");
        assert_eq!(v["branch"], "fix-scorer");
        assert_eq!(v["prompt"], "rewrite it");
    }

    #[test]
    fn a_prompt_may_contain_colons() {
        // Prompts routinely do: "fix: the thing". Splitting from the left
        // twice is what keeps that working.
        let v = parse_target("/code/acme:b:fix: the thing, then: ship").expect("parse");
        assert_eq!(v["branch"], "b");
        assert_eq!(v["prompt"], "fix: the thing, then: ship");
    }

    #[test]
    fn a_malformed_target_is_refused() {
        assert!(parse_target("just-a-word").is_err());
        assert!(parse_target("/code/acme:branch-only").is_err());
        assert!(parse_target("/code/acme::no branch").is_err());
        assert!(parse_target("/code/acme:b:   ").is_err());
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
}
