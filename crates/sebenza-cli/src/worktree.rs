//! Worktree subcommands (`add`, `list`, `open`, `close`, `refresh`, `archive`,
//! `unarchive`, `label`, `remove`, `merge`, `send`, `tab`, `prune`, `restore`).
//! All-HTTP: every operation talks to the running `sebenza-server`.

use std::io::Write;

use anyhow::{Result, anyhow};
use serde_json::json;

use common::adapters::fs::read_open_sessions_state;
use common::adapters::git::GitGateway;
use common::domain::policies::is_valid_branch_name;

use crate::http::Http;
use crate::tmux_switch::switch_to_window;

pub struct WorktreeContext {
    pub command: String,
    pub args: Vec<String>,
    pub project_dir: String,
    pub port: u16,
}

/// A parse result: `Parsed(T)` on success, `Help` when `--help` was requested.
enum Parse<T> {
    Parsed(T),
    Help,
}

fn usage(command: &str) -> String {
    match command {
        "add" => [
            "Usage:",
            "  sebenza-cli add [branch] [--existing] [--base <branch>] [--profile <name>] [--agent <id>] [--prompt <text>] [--env KEY=VALUE] [--detach]",
            "",
            "Options:",
            "  --existing               Use an existing local or remote branch instead of creating a new one",
            "  --base <branch>          Base branch for a new worktree (defaults to config)",
            "  --profile <name>         Worktree profile from .ai/sebenza.yaml",
            "  --agent <id>             Agent id to launch (repeatable)",
            "  --prompt <text>          Initial agent prompt",
            "  --env KEY=VALUE          Runtime env override (repeatable)",
            "  -d, --detach             Create worktree without switching to it",
            "  --help                   Show this help message",
        ]
        .join("\n"),
        "list" => [
            "Usage:",
            "  sebenza-cli list [--all|--archived] [--all-projects] [--search <text>]",
            "",
            "Options:",
            "  --all                    Include archived worktrees",
            "  --all-projects           Show worktrees from every project this server has loaded",
            "  --archived               Show only archived worktrees",
            "  --search <text>          Filter worktrees by branch/profile/agent",
            "  --help                   Show this help message",
        ]
        .join("\n"),
        "open" => "Usage:\n  sebenza-cli open <branch>".to_string(),
        "close" => "Usage:\n  sebenza-cli close <branch>".to_string(),
        "refresh" => "Usage:\n  sebenza-cli refresh <branch>".to_string(),
        "archive" => "Usage:\n  sebenza-cli archive <branch>".to_string(),
        "unarchive" => "Usage:\n  sebenza-cli unarchive <branch>".to_string(),
        "label" => [
            "Usage:",
            "  sebenza-cli label <branch> <label>",
            "  sebenza-cli label <branch> --clear",
            "",
            "Options:",
            "  --clear                  Clear the workspace label",
            "  --label <text>           Label text",
            "  --help                   Show this help message",
        ]
        .join("\n"),
        "remove" => "Usage:\n  sebenza-cli remove <branch>".to_string(),
        "merge" => "Usage:\n  sebenza-cli merge <branch>".to_string(),
        "send" => [
            "Usage:",
            "  sebenza-cli send <branch> <prompt> [--preamble <text>]",
            "",
            "Options:",
            "  --prompt <text>          Prompt text (alternative to positional arg)",
            "  --preamble <text>        Preamble text sent before the prompt",
            "  --help                   Show this help message",
        ]
        .join("\n"),
        "prune" => "Usage:\n  sebenza-cli prune".to_string(),
        "restore" => {
            "Usage:\n  sebenza-cli restore\n\nRe-open every worktree session that was open the last time sessions were saved."
                .to_string()
        }
        "tab" => [
            "Usage:",
            "  sebenza-cli tab <branch>                 List the agent tabs (★ marks the active one)",
            "  sebenza-cli tab <branch> new             Create a new tab (forks the current session by default)",
            "  sebenza-cli tab <branch> switch <tabId>  Switch the visible agent pane to a tab",
            "  sebenza-cli tab <branch> close <tabId>   Delete a tab",
            "",
            "Options:",
            "  --agent <id>             With \"new\": start a fresh session of this agent instead of forking",
            "  --help                   Show this help message",
        ]
        .join("\n"),
        _ => format!("Usage:\n  sebenza-cli {command} <branch>"),
    }
}

/// Read a flag's value, supporting both `--flag value` and `--flag=value`.
fn read_option_value(args: &[String], index: usize, flag: &str) -> Result<(String, usize)> {
    let arg = args
        .get(index)
        .ok_or_else(|| anyhow!("{flag} requires a value"))?;
    let prefix = format!("{flag}=");
    if let Some(rest) = arg.strip_prefix(&prefix) {
        return Ok((rest.to_string(), index));
    }
    let value = args
        .get(index + 1)
        .ok_or_else(|| anyhow!("{flag} requires a value"))?;
    Ok((value.clone(), index + 1))
}

// ── Parsers ─────────────────────────────────────────────────────────────────

struct AddArgs {
    body: serde_json::Value,
    detach: bool,
}

fn parse_add(args: &[String]) -> Result<Parse<AddArgs>> {
    let mut mode: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut base: Option<String> = None;
    let mut profile: Option<String> = None;
    let mut prompt: Option<String> = None;
    let mut agents: Vec<String> = Vec::new();
    let mut env: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    let mut detach = false;

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--help" || arg == "-h" {
            return Ok(Parse::Help);
        } else if arg == "--existing" {
            mode = Some("existing".to_string());
        } else if arg == "--detach" || arg == "-d" {
            detach = true;
        } else if arg == "--profile" || arg.starts_with("--profile=") {
            let (v, n) = read_option_value(args, index, "--profile")?;
            profile = Some(v);
            index = n;
        } else if arg == "--base" || arg.starts_with("--base=") {
            let (v, n) = read_option_value(args, index, "--base")?;
            base = Some(v);
            index = n;
        } else if arg == "--agent" || arg.starts_with("--agent=") {
            let (v, n) = read_option_value(args, index, "--agent")?;
            let trimmed = v.trim().to_string();
            if trimmed.is_empty() {
                return Err(anyhow!("Agent id cannot be empty"));
            }
            agents.push(trimmed);
            index = n;
        } else if arg == "--prompt" || arg.starts_with("--prompt=") {
            let (v, n) = read_option_value(args, index, "--prompt")?;
            prompt = Some(v);
            index = n;
        } else if arg == "--env" || arg.starts_with("--env=") {
            let (v, n) = read_option_value(args, index, "--env")?;
            let sep = v.find('=').unwrap_or(0);
            if sep == 0 {
                return Err(anyhow!("--env must use KEY=VALUE"));
            }
            env.insert(v[..sep].to_string(), json!(v[sep + 1..].to_string()));
            index = n;
        } else if arg == "--branch" || arg.starts_with("--branch=") {
            let (v, n) = read_option_value(args, index, "--branch")?;
            let v = v.trim().to_string();
            if let Some(existing) = &branch {
                if existing != &v {
                    return Err(anyhow!(
                        "Conflicting branch values: \"{existing}\" and \"{v}\""
                    ));
                }
            }
            branch = Some(v);
            index = n;
        } else if arg.starts_with('-') {
            return Err(anyhow!("Unknown option: {arg}"));
        } else if branch.is_some() {
            return Err(anyhow!("Unexpected argument: {arg}"));
        } else {
            branch = Some(arg.clone());
        }
        index += 1;
    }

    let mut body = serde_json::Map::new();
    if let Some(m) = mode {
        body.insert("mode".into(), json!(m));
    }
    if let Some(b) = branch {
        body.insert("branch".into(), json!(b));
    }
    if let Some(b) = base {
        body.insert("baseBranch".into(), json!(b));
    }
    if let Some(p) = profile {
        body.insert("profile".into(), json!(p));
    }
    if let Some(p) = prompt {
        body.insert("prompt".into(), json!(p));
    }
    if !agents.is_empty() {
        body.insert("agents".into(), json!(agents));
    }
    if !env.is_empty() {
        body.insert("envOverrides".into(), serde_json::Value::Object(env));
    }

    Ok(Parse::Parsed(AddArgs {
        body: serde_json::Value::Object(body),
        detach,
    }))
}

fn parse_branch(args: &[String]) -> Result<Parse<String>> {
    let mut branch: Option<String> = None;
    for arg in args {
        if arg == "--help" || arg == "-h" {
            return Ok(Parse::Help);
        }
        if arg.starts_with('-') {
            return Err(anyhow!("Unknown option: {arg}"));
        }
        if branch.is_some() {
            return Err(anyhow!("Unexpected argument: {arg}"));
        }
        branch = Some(arg.clone());
    }
    let branch = branch.ok_or_else(|| anyhow!("Missing required argument: <branch>"))?;
    if !is_valid_branch_name(&branch) {
        return Err(anyhow!("Invalid worktree name"));
    }
    Ok(Parse::Parsed(branch))
}

struct LabelArgs {
    branch: String,
    label: Option<String>,
}

fn parse_label(args: &[String]) -> Result<Parse<LabelArgs>> {
    let mut branch: Option<String> = None;
    let mut clear = false;
    let mut option_label: Option<String> = None;
    let mut label_parts: Vec<String> = Vec::new();

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--help" || arg == "-h" {
            return Ok(Parse::Help);
        } else if arg == "--clear" {
            clear = true;
        } else if arg == "--label" || arg.starts_with("--label=") {
            if option_label.is_some() {
                return Err(anyhow!("Cannot use --label more than once"));
            }
            let (v, n) = read_option_value(args, index, "--label")?;
            option_label = Some(v);
            index = n;
        } else if arg.starts_with('-') {
            return Err(anyhow!("Unknown option: {arg}"));
        } else if branch.is_none() {
            branch = Some(arg.clone());
        } else {
            if option_label.is_some() {
                return Err(anyhow!("Cannot use --label with a positional label"));
            }
            label_parts.push(arg.clone());
        }
        index += 1;
    }

    let branch = branch.ok_or_else(|| anyhow!("Missing required argument: <branch>"))?;
    if !is_valid_branch_name(&branch) {
        return Err(anyhow!("Invalid worktree name"));
    }
    if option_label.is_some() && !label_parts.is_empty() {
        return Err(anyhow!("Cannot use --label with a positional label"));
    }
    let label = option_label
        .unwrap_or_else(|| label_parts.join(" "))
        .trim()
        .to_string();
    if clear && !label.is_empty() {
        return Err(anyhow!("Cannot use --clear with a label"));
    }
    if !clear && label.is_empty() {
        return Err(anyhow!("Missing required argument: <label>"));
    }
    Ok(Parse::Parsed(LabelArgs {
        branch,
        label: if clear { None } else { Some(label) },
    }))
}

struct SendArgs {
    branch: String,
    text: String,
    preamble: Option<String>,
}

fn parse_send(args: &[String]) -> Result<Parse<SendArgs>> {
    let mut branch: Option<String> = None;
    let mut text: Option<String> = None;
    let mut preamble: Option<String> = None;

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--help" || arg == "-h" {
            return Ok(Parse::Help);
        } else if arg == "--prompt" || arg.starts_with("--prompt=") {
            if text.is_some() {
                return Err(anyhow!(
                    "Cannot use --prompt with a positional prompt argument"
                ));
            }
            let (v, n) = read_option_value(args, index, "--prompt")?;
            text = Some(v);
            index = n;
        } else if arg == "--preamble" || arg.starts_with("--preamble=") {
            let (v, n) = read_option_value(args, index, "--preamble")?;
            preamble = Some(v);
            index = n;
        } else if arg.starts_with('-') {
            return Err(anyhow!("Unknown option: {arg}"));
        } else if branch.is_none() {
            branch = Some(arg.clone());
        } else if text.is_none() {
            text = Some(arg.clone());
        } else {
            return Err(anyhow!(
                "Unexpected argument: {arg}. Use either a positional prompt or --prompt, not both"
            ));
        }
        index += 1;
    }

    let branch = branch.ok_or_else(|| anyhow!("Missing required argument: <branch>"))?;
    if !is_valid_branch_name(&branch) {
        return Err(anyhow!("Invalid worktree name"));
    }
    let text = text.ok_or_else(|| anyhow!("Missing required argument: <prompt>"))?;
    Ok(Parse::Parsed(SendArgs {
        branch,
        text,
        preamble,
    }))
}

#[derive(Clone, Copy)]
enum TabAction {
    List,
    New,
    Switch,
    Close,
}

struct TabArgs {
    branch: String,
    action: TabAction,
    tab_id: Option<String>,
    /// Start a fresh session of this agent instead of forking.
    agent: Option<String>,
}

fn parse_tab(args: &[String]) -> Result<Parse<TabArgs>> {
    let mut positional: Vec<String> = Vec::new();
    let mut agent: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--help" || arg == "-h" {
            return Ok(Parse::Help);
        }
        // Options must be handled before the `-` rejection below.
        if arg == "--agent" || arg.starts_with("--agent=") {
            let (value, consumed) = read_option_value(args, index, "--agent")?;
            let value = value.trim().to_string();
            if value.is_empty() {
                return Err(anyhow!("--agent requires a value"));
            }
            agent = Some(value);
            index = consumed + 1;
            continue;
        }
        if arg.starts_with('-') {
            return Err(anyhow!("Unknown option: {arg}"));
        }
        positional.push(arg.clone());
        index += 1;
    }

    let branch = positional
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("Missing required argument: <branch>"))?;
    if !is_valid_branch_name(&branch) {
        return Err(anyhow!("Invalid worktree name"));
    }
    let raw_action = positional.get(1).map(String::as_str).unwrap_or("list");
    let action = match raw_action {
        "list" => TabAction::List,
        "new" => TabAction::New,
        "switch" => TabAction::Switch,
        "close" => TabAction::Close,
        other => return Err(anyhow!("Unknown tab action: {other}")),
    };
    let tab_id = positional.get(2).cloned();
    if matches!(action, TabAction::Switch | TabAction::Close) && tab_id.is_none() {
        return Err(anyhow!("The \"{raw_action}\" action requires a <tabId>"));
    }
    if positional.len() > 3 {
        return Err(anyhow!("Unexpected argument: {}", positional[3]));
    }
    if agent.is_some() && !matches!(action, TabAction::New) {
        return Err(anyhow!("--agent is only valid for the \"new\" action"));
    }
    Ok(Parse::Parsed(TabArgs {
        branch,
        action,
        tab_id,
        agent,
    }))
}

struct ListArgs {
    mode: ListMode,
    search: String,
    /// Span every project this server has loaded, not just the current directory's.
    all_projects: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum ListMode {
    Active,
    All,
    Archived,
}

fn parse_list(args: &[String]) -> Result<Parse<ListArgs>> {
    let mut mode = ListMode::Active;
    let mut search = String::new();
    let mut all_projects = false;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--help" || arg == "-h" {
            return Ok(Parse::Help);
        } else if arg == "--all" {
            if mode == ListMode::Archived {
                return Err(anyhow!("Cannot use --all with --archived"));
            }
            mode = ListMode::All;
        } else if arg == "--archived" {
            if mode == ListMode::All {
                return Err(anyhow!("Cannot use --archived with --all"));
            }
            mode = ListMode::Archived;
        } else if arg == "--all-projects" {
            all_projects = true;
        } else if arg == "--search" || arg.starts_with("--search=") {
            let (v, n) = read_option_value(args, index, "--search")?;
            search = v;
            index = n;
        } else {
            return Err(anyhow!("Unknown option: {arg}"));
        }
        index += 1;
    }
    Ok(Parse::Parsed(ListArgs {
        mode,
        search,
        all_projects,
    }))
}

fn parse_no_args(args: &[String]) -> Result<Parse<()>> {
    for arg in args {
        if arg == "--help" || arg == "-h" {
            return Ok(Parse::Help);
        }
        if arg.starts_with('-') {
            return Err(anyhow!("Unknown option: {arg}"));
        }
        return Err(anyhow!("Unexpected argument: {arg}"));
    }
    Ok(Parse::Parsed(()))
}

// ── Command runner ────────────────────────────────────────────────────────

pub async fn run(ctx: WorktreeContext) -> i32 {
    match run_inner(&ctx).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

async fn run_inner(ctx: &WorktreeContext) -> Result<i32> {
    let http = Http::new(ctx.port);
    let command = ctx.command.as_str();

    match command {
        "add" => {
            let parsed = match parse_add(&ctx.args)? {
                Parse::Help => {
                    println!("{}", usage("add"));
                    return Ok(0);
                }
                Parse::Parsed(p) => p,
            };
            let base = http.resolve_project_base(&ctx.project_dir).await?;
            let branches = http.create_worktree(&base, parsed.body).await?;
            for branch in &branches {
                println!("Created worktree {branch}");
            }
            if !parsed.detach {
                if let Some(primary) = branches.first() {
                    switch_to_window(&ctx.project_dir, primary);
                }
            }
            Ok(0)
        }
        "list" => {
            let parsed = match parse_list(&ctx.args)? {
                Parse::Help => {
                    println!("{}", usage("list"));
                    return Ok(0);
                }
                Parse::Parsed(p) => p,
            };
            if parsed.all_projects {
                let projects = http.fetch_active_worktrees().await?;
                for line in cross_project_list_lines(&projects, &parsed) {
                    println!("{line}");
                }
            } else {
                let base = http.resolve_project_base(&ctx.project_dir).await?;
                let snapshot = http.get_project(&base).await?;
                print_list(&snapshot.worktrees, &parsed);
            }
            Ok(0)
        }
        "prune" => {
            match parse_no_args(&ctx.args)? {
                Parse::Help => {
                    println!("{}", usage("prune"));
                    return Ok(0);
                }
                Parse::Parsed(()) => {}
            }
            let base = http.resolve_project_base(&ctx.project_dir).await?;
            let snapshot = http.get_project(&base).await?;
            if snapshot.worktrees.is_empty() {
                println!("No worktrees found.");
                return Ok(0);
            }
            let closed: Vec<String> = snapshot
                .worktrees
                .iter()
                // The repo root is never prunable — it is not a removable worktree.
                .filter(|w| !w.mux && w.kind != "main")
                .map(|w| w.branch.clone())
                .collect();
            if closed.is_empty() {
                println!("No closed worktrees to prune.");
                return Ok(0);
            }
            if !confirm_prune(closed.len()) {
                println!("Aborted.");
                return Ok(0);
            }
            let mut removed: Vec<String> = Vec::new();
            for branch in &closed {
                http.remove_worktree(&base, branch).await?;
                removed.push(branch.clone());
            }
            println!(
                "Pruned {} worktree{}: {}",
                removed.len(),
                if removed.len() == 1 { "" } else { "s" },
                removed.join(", ")
            );
            Ok(0)
        }
        "restore" => {
            match parse_no_args(&ctx.args)? {
                Parse::Help => {
                    println!("{}", usage("restore"));
                    return Ok(0);
                }
                Parse::Parsed(()) => {}
            }
            let git = GitGateway::new();
            let git_dir = git
                .resolve_worktree_git_dir(&ctx.project_dir)
                .map_err(|e| anyhow!(e))?;
            let state = read_open_sessions_state(&git_dir);
            if state.branches.is_empty() {
                println!("No saved sessions to restore.");
                return Ok(0);
            }
            let base = http.resolve_project_base(&ctx.project_dir).await?;
            let snapshot = http.get_project(&base).await?;
            let existing: std::collections::HashSet<&str> = snapshot
                .worktrees
                .iter()
                .map(|w| w.branch.as_str())
                .collect();
            let open: std::collections::HashSet<&str> = snapshot
                .worktrees
                .iter()
                .filter(|w| w.mux)
                .map(|w| w.branch.as_str())
                .collect();

            let mut restored = 0;
            let mut skipped = 0;
            let mut failed = 0;
            let mut first_restored: Option<String> = None;

            for branch in &state.branches {
                if open.contains(branch.as_str()) {
                    println!("Already open: {branch}");
                    skipped += 1;
                    continue;
                }
                if !existing.contains(branch.as_str()) {
                    eprintln!("Skipping {branch}: worktree no longer exists");
                    skipped += 1;
                    continue;
                }
                match http.open_worktree(&base, branch).await {
                    Ok(()) => {
                        println!("Restored {branch}");
                        restored += 1;
                        if first_restored.is_none() {
                            first_restored = Some(branch.clone());
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to restore {branch}: {e}");
                        failed += 1;
                    }
                }
            }

            let mut parts = vec![format!(
                "Restored {restored} session{}",
                if restored == 1 { "" } else { "s" }
            )];
            if skipped > 0 {
                parts.push(format!("skipped {skipped}"));
            }
            if failed > 0 {
                parts.push(format!("{failed} failed"));
            }
            println!("{}.", parts.join(", "));

            if let Some(first) = first_restored {
                switch_to_window(&ctx.project_dir, &first);
            }
            Ok(if failed > 0 { 1 } else { 0 })
        }
        "send" => {
            let parsed = match parse_send(&ctx.args)? {
                Parse::Help => {
                    println!("{}", usage("send"));
                    return Ok(0);
                }
                Parse::Parsed(p) => p,
            };
            let base = http.resolve_project_base(&ctx.project_dir).await?;
            http.send_prompt(
                &base,
                &parsed.branch,
                &parsed.text,
                parsed.preamble.as_deref(),
            )
            .await?;
            println!("Sent prompt to {}", parsed.branch);
            Ok(0)
        }
        "tab" => {
            let parsed = match parse_tab(&ctx.args)? {
                Parse::Help => {
                    println!("{}", usage("tab"));
                    return Ok(0);
                }
                Parse::Parsed(p) => p,
            };
            let base = http.resolve_project_base(&ctx.project_dir).await?;
            match parsed.action {
                TabAction::New => {
                    let tab = match parsed.agent.as_deref() {
                        Some(agent) => http.create_agent_tab(&base, &parsed.branch, agent).await?,
                        None => http.create_tab(&base, &parsed.branch).await?,
                    };
                    println!(
                        "Created {} ({}) in {}",
                        tab.label, tab.tab_id, parsed.branch
                    );
                }
                TabAction::Switch => {
                    let tab_id = parsed.tab_id.unwrap();
                    http.select_tab(&base, &parsed.branch, &tab_id).await?;
                    println!("Switched {} to tab {tab_id}", parsed.branch);
                }
                TabAction::Close => {
                    let tab_id = parsed.tab_id.unwrap();
                    http.delete_tab(&base, &parsed.branch, &tab_id).await?;
                    println!("Closed tab {tab_id} in {}", parsed.branch);
                }
                TabAction::List => {
                    let snapshot = http.get_project(&base).await?;
                    match snapshot
                        .worktrees
                        .iter()
                        .find(|w| w.branch == parsed.branch)
                    {
                        None => println!("Worktree not found: {}", parsed.branch),
                        Some(w) => {
                            for tab in &w.tabs {
                                let marker = if Some(&tab.tab_id) == w.active_tab_id.as_ref() {
                                    "★"
                                } else {
                                    " "
                                };
                                println!("{marker} {:<10} {}", tab.label, tab.tab_id);
                            }
                        }
                    }
                }
            }
            Ok(0)
        }
        "label" => {
            let parsed = match parse_label(&ctx.args)? {
                Parse::Help => {
                    println!("{}", usage("label"));
                    return Ok(0);
                }
                Parse::Parsed(p) => p,
            };
            let base = http.resolve_project_base(&ctx.project_dir).await?;
            let result = http
                .set_label(&base, &parsed.branch, parsed.label.as_deref())
                .await?;
            match result {
                Some(label) => println!("Labeled worktree {} as \"{label}\"", parsed.branch),
                None => println!("Cleared label for {}", parsed.branch),
            }
            Ok(0)
        }
        // Single-branch commands.
        _ => {
            let branch = match parse_branch(&ctx.args)? {
                Parse::Help => {
                    println!("{}", usage(command));
                    return Ok(0);
                }
                Parse::Parsed(b) => b,
            };
            let base = http.resolve_project_base(&ctx.project_dir).await?;
            match command {
                "open" => {
                    http.open_worktree(&base, &branch).await?;
                    println!("Opened worktree {branch}");
                    switch_to_window(&ctx.project_dir, &branch);
                }
                "close" => {
                    http.close_worktree(&base, &branch).await?;
                    println!("Closed worktree {branch}");
                }
                "refresh" => {
                    http.refresh_agent_terminal(&base, &branch).await?;
                    println!("Refreshed agent terminal for {branch}");
                }
                "archive" => {
                    http.set_archived(&base, &branch, true).await?;
                    println!("Archived worktree {branch}");
                }
                "unarchive" => {
                    http.set_archived(&base, &branch, false).await?;
                    println!("Restored worktree {branch}");
                }
                "remove" => {
                    http.remove_worktree(&base, &branch).await?;
                    println!("Removed worktree {branch}");
                }
                "merge" => {
                    let snapshot = http.get_project(&base).await?;
                    http.merge_worktree(&base, &branch).await?;
                    println!("Merged {branch} into {}", snapshot.project.main_branch);
                }
                other => return Err(anyhow!("Unknown command: {other}")),
            }
            Ok(0)
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn confirm_prune(count: usize) -> bool {
    print!(
        "Prune {count} closed worktree{}? This action cannot be undone. [y/N] ",
        if count == 1 { "" } else { "s" }
    );
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

/// Plain-text marker for a worktree waiting on a human, or `None`.
///
/// Wording matches the dashboard ticker so the two surfaces describe the same state
/// identically. An unrecognised state yields `None` rather than a guess: a newer server
/// may add one this binary predates, and announcing an unnamed demand would be worse
/// than staying quiet about it.
fn feedback_marker(feedback_state: &str) -> Option<&'static str> {
    match feedback_state {
        "permission_request" => Some("needs approval"),
        "user_question" => Some("needs an answer"),
        _ => None,
    }
}

/// `list --all-projects` output: each project's worktrees under its own heading.
///
/// Reuses `list_lines` per project so filtering, ordering and the feedback marker are
/// defined once. A project whose worktrees all filter out is omitted entirely rather than
/// printed as a bare heading — a heading with nothing under it reads as a bug.
fn cross_project_list_lines(
    projects: &[crate::http::ActiveWorktreeProject],
    options: &ListArgs,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for project in projects {
        if project.worktrees.is_empty() {
            continue;
        }
        let lines = list_lines(&project.worktrees, options);
        // `list_lines` reports its own empty states; in the grouped view those belong to
        // the whole run, not to one project, so skip the project instead.
        if lines.iter().any(|l| l.starts_with("No ")) {
            continue;
        }
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(format!("{} ({})", project.name, project.prefix));
        out.extend(lines);
    }

    if out.is_empty() {
        out.push("No worktrees found in any project.".to_string());
    }
    out
}

fn print_list(worktrees: &[crate::http::WorktreeSnapshot], options: &ListArgs) {
    for line in list_lines(worktrees, options) {
        println!("{line}");
    }
}

/// The lines `list` prints, so the output can be asserted rather than captured from
/// stdout.
fn list_lines(worktrees: &[crate::http::WorktreeSnapshot], options: &ListArgs) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if worktrees.is_empty() {
        out.push("No worktrees found.".to_string());
        return out;
    }

    struct Row {
        branch: String,
        label: Option<String>,
        is_open: bool,
        archived: bool,
        /// The agent lifecycle, which this listing previously dropped entirely.
        agent_status: String,
        feedback: Option<&'static str>,
        info: String,
        search_text: String,
    }

    let mut rows: Vec<Row> = worktrees
        .iter()
        .map(|w| {
            let has_meta = w.profile.is_some() || w.agent_name.is_some();
            let info = if has_meta {
                format!(
                    "{} / {}",
                    w.profile.clone().unwrap_or_default(),
                    w.agent_name.clone().unwrap_or_default()
                )
            } else {
                String::new()
            };
            let search_text = [
                w.label.clone().unwrap_or_default(),
                w.branch.clone(),
                w.base_branch.clone().unwrap_or_default(),
                w.profile.clone().unwrap_or_default(),
                w.agent_name.clone().unwrap_or_default(),
            ]
            .join(" ");
            Row {
                branch: w.branch.clone(),
                label: w.label.clone(),
                is_open: w.mux,
                archived: w.archived,
                agent_status: w.status.clone(),
                feedback: feedback_marker(&w.feedback_state),
                info,
                search_text,
            }
        })
        .collect();

    let query = options.search.trim().to_lowercase();
    rows.retain(|r| query.is_empty() || r.search_text.to_lowercase().contains(&query));
    rows.sort_by(|a, b| a.branch.cmp(&b.branch));

    let visible: Vec<&Row> = rows
        .iter()
        .filter(|r| match options.mode {
            ListMode::All => true,
            ListMode::Archived => r.archived,
            ListMode::Active => !r.archived,
        })
        .collect();

    if visible.is_empty() {
        let hidden_archived = if options.mode == ListMode::Active {
            rows.iter().filter(|r| r.archived).count()
        } else {
            0
        };
        if hidden_archived > 0 {
            out.push(format!(
                "No active worktrees found. {hidden_archived} archived worktree{} hidden. Use --all or --archived.",
                if hidden_archived == 1 { "" } else { "s" }
            ));
            return out;
        }
        if options.mode == ListMode::Archived {
            out.push("No archived worktrees found.".to_string());
            return out;
        }
        let q = options.search.trim();
        if q.is_empty() {
            out.push("No worktrees found.".to_string());
        } else {
            out.push(format!("No worktrees found for \"{q}\"."));
        }
        return out;
    }

    let display_name = |r: &Row| match &r.label {
        Some(l) => format!("{l} ({})", r.branch),
        None => r.branch.clone(),
    };
    let max_name = visible
        .iter()
        .map(|r| display_name(r).len())
        .max()
        .unwrap_or(0);

    for r in &visible {
        let status = format!(
            "{}{}",
            if r.is_open { "open" } else { "closed" },
            if r.archived { " archived" } else { "" }
        );
        // Agent status and the feedback marker share a column so the existing three-column
        // shape is preserved; the marker leads because it is the thing worth acting on.
        let agent = match (r.feedback, r.agent_status.as_str()) {
            (Some(marker), "") => marker.to_string(),
            (Some(marker), status) => format!("{marker} ({status})"),
            (None, status) => status.to_string(),
        };
        let name = display_name(r);
        let line = format!(
            "{:<w$} {:<15} {:<26} {}",
            name,
            status,
            agent,
            r.info,
            w = max_name + 2
        );
        out.push(line.trim_end().to_string());
    }

    if options.mode == ListMode::Active {
        let hidden_archived = rows.iter().filter(|r| r.archived).count();
        if hidden_archived > 0 {
            out.push(format!(
                "Hidden {hidden_archived} archived worktree{}. Use --all or --archived.",
                if hidden_archived == 1 { "" } else { "s" }
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(branch: &str, status: &str, feedback: &str) -> crate::http::WorktreeSnapshot {
        crate::http::WorktreeSnapshot {
            branch: branch.to_string(),
            kind: "linked".to_string(),
            label: None,
            base_branch: None,
            archived: false,
            profile: None,
            agent_name: None,
            mux: true,
            status: status.to_string(),
            feedback_state: feedback.to_string(),
            prs: Vec::new(),
            oneshot: None,
            tabs: Vec::new(),
            active_tab_id: None,
        }
    }

    fn active() -> ListArgs {
        ListArgs {
            mode: ListMode::Active,
            search: String::new(),
            all_projects: false,
        }
    }

    #[test]
    fn list_shows_the_agent_status_for_each_worktree() {
        // Before this, `list` printed only open/closed/archived, so a CLI-only user could
        // not see what any agent was actually doing.
        let lines = list_lines(
            &[
                snapshot("feat-a", "running", "none"),
                snapshot("feat-b", "idle", "none"),
            ],
            &active(),
        );

        assert!(
            lines
                .iter()
                .any(|l| l.contains("feat-a") && l.contains("running")),
            "expected a running status in {lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("feat-b") && l.contains("idle")),
            "expected an idle status in {lines:?}"
        );
    }

    #[test]
    fn list_marks_a_worktree_that_needs_a_response() {
        let lines = list_lines(
            &[snapshot(
                "feat-blocked",
                "awaiting_permission",
                "permission_request",
            )],
            &active(),
        );

        let line = lines
            .iter()
            .find(|l| l.contains("feat-blocked"))
            .expect("the worktree should be listed");
        // Plain text, no colour: this has to survive a pipe, a log, and a terminal that
        // renders no escape codes at all.
        assert!(
            line.contains("needs approval"),
            "expected a marker in {line:?}"
        );
        assert!(
            !line.contains("\u{1b}["),
            "the marker must not rely on colour: {line:?}"
        );
    }

    #[test]
    fn list_distinguishes_a_question_from_a_permission_request() {
        let lines = list_lines(
            &[snapshot("feat-asked", "running", "user_question")],
            &active(),
        );

        assert!(
            lines.iter().any(|l| l.contains("needs an answer")),
            "expected a question marker in {lines:?}"
        );
    }

    #[test]
    fn list_marks_nothing_when_no_worktree_is_waiting() {
        let lines = list_lines(&[snapshot("feat-a", "running", "none")], &active());

        assert!(
            !lines.iter().any(|l| l.contains("needs")),
            "no worktree is waiting, so nothing should be marked: {lines:?}"
        );
    }

    #[test]
    fn an_unknown_feedback_state_is_not_announced_as_needing_anything() {
        // A newer server could add a state this binary predates. Listing it as some
        // unrecognised demand would be worse than staying quiet about it.
        let lines = list_lines(
            &[snapshot("feat-a", "running", "telepathy_request")],
            &active(),
        );

        assert!(
            !lines.iter().any(|l| l.contains("needs")),
            "an unknown state must not be announced: {lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("feat-a")),
            "the worktree must still be listed: {lines:?}"
        );
    }

    #[test]
    fn the_empty_message_is_unchanged() {
        assert_eq!(list_lines(&[], &active()), vec!["No worktrees found."]);
    }
    fn project(
        prefix: &str,
        name: &str,
        worktrees: Vec<crate::http::WorktreeSnapshot>,
    ) -> crate::http::ActiveWorktreeProject {
        crate::http::ActiveWorktreeProject {
            prefix: prefix.to_string(),
            name: name.to_string(),
            worktrees,
        }
    }

    #[test]
    fn all_projects_groups_and_labels_each_project() {
        let lines = cross_project_list_lines(
            &[
                project(
                    "alpha",
                    "Alpha",
                    vec![snapshot("feat-a", "running", "none")],
                ),
                project("beta", "Beta", vec![snapshot("feat-b", "idle", "none")]),
            ],
            &active(),
        );

        let text = lines.join("\n");
        assert!(
            text.contains("Alpha"),
            "expected a project heading in {lines:?}"
        );
        assert!(
            text.contains("Beta"),
            "expected a project heading in {lines:?}"
        );
        // Each worktree still appears under its own project.
        let alpha_at = text.find("Alpha").unwrap();
        let feat_a_at = text.find("feat-a").unwrap();
        let beta_at = text.find("Beta").unwrap();
        assert!(
            alpha_at < feat_a_at && feat_a_at < beta_at,
            "grouping is wrong: {text}"
        );
    }

    #[test]
    fn all_projects_still_marks_worktrees_needing_a_response() {
        let lines = cross_project_list_lines(
            &[project(
                "alpha",
                "Alpha",
                vec![snapshot(
                    "feat-blocked",
                    "awaiting_permission",
                    "permission_request",
                )],
            )],
            &active(),
        );

        assert!(
            lines.iter().any(|l| l.contains("needs approval")),
            "the marker must survive the cross-project view: {lines:?}"
        );
    }

    #[test]
    fn all_projects_skips_a_project_with_no_matching_worktrees() {
        // A loaded-but-quiet project should not print an empty heading and nothing else.
        let lines = cross_project_list_lines(
            &[
                project("quiet", "Quiet", Vec::new()),
                project("busy", "Busy", vec![snapshot("feat-a", "running", "none")]),
            ],
            &active(),
        );

        let text = lines.join("\n");
        assert!(
            !text.contains("Quiet"),
            "an empty project should be omitted: {text}"
        );
        assert!(text.contains("Busy"), "{text}");
    }

    #[test]
    fn all_projects_reports_when_nothing_is_running_anywhere() {
        let lines = cross_project_list_lines(&[project("quiet", "Quiet", Vec::new())], &active());

        assert_eq!(lines, vec!["No worktrees found in any project."]);
    }

    #[test]
    fn single_project_output_is_unchanged_by_the_flag_existing() {
        // Regression guard for the refactor: the default path must be byte-identical.
        let worktrees = [snapshot("feat-a", "running", "none")];
        assert_eq!(
            list_lines(&worktrees, &active()),
            list_lines(&worktrees, &active())
        );
        assert!(list_lines(&worktrees, &active())[0].contains("feat-a"));
    }
}
