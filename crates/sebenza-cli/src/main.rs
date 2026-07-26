//! Sebenza CLI (`sebenza-cli`). Most commands talk to a running `sebenza-server`
//! over HTTP; `serve`, `init`, `service`, `update`, and `completion` run locally.

mod completions;
mod env_files;
mod http;
mod init;
mod migrate;
mod oneshot;
mod port;
mod project;
mod serve;
mod service;
mod service_units;
mod tmux_switch;
mod update;
mod worktree;

use port::resolve_live_server_port;

const DEFAULT_PORT: u16 = 5111;

fn usage() {
    println!(
        r#"
sebenza-cli — Sebenza: manage Git worktrees with AI coding agents

Usage:
  sebenza-cli serve        Start the dashboard server (--app opens in app mode)
  sebenza-cli init         Interactive project setup
  sebenza-cli service      Manage sebenza-cli as a system service
  sebenza-cli update       Update sebenza-cli to the latest version
  sebenza-cli add          Create a worktree using the dashboard lifecycle
  sebenza-cli oneshot      Run a worktree start-to-finish, streaming logs to stdout
  sebenza-cli list         List worktrees and their status
  sebenza-cli open         Open an existing worktree session
  sebenza-cli close        Close a worktree session without removing it
  sebenza-cli refresh      Refresh a Codex agent terminal from saved chat
  sebenza-cli archive      Hide a worktree from the default list
  sebenza-cli unarchive    Show an archived worktree again
  sebenza-cli label        Set or clear a workspace label
  sebenza-cli remove       Remove a worktree
  sebenza-cli merge        Merge a worktree into the main branch and remove it
  sebenza-cli send         Send a prompt to a running worktree agent
  sebenza-cli tab          List, create, switch, or close agent tabs in a worktree
  sebenza-cli prune        Remove all closed (not open) worktrees in the current project
  sebenza-cli restore      Re-open all worktree sessions that were open before
  sebenza-cli project      List, add, or remove projects served by the dashboard
  sebenza-cli completion   Generate shell completion script (bash, zsh)

Options:
  --port N         Set port (default: 5111).
                   Without --port, CLI commands target the live server for this project.
  --app            Open dashboard in browser app mode (minimal window)
  --debug          Show debug-level logs
  --version        Show version number
  --help           Show this help message

Environment:
  PORT             Same as --port (flag takes precedence)
"#
    );
}

const ROOT_COMMANDS: &[&str] = &[
    "serve", "init", "service", "update", "add", "oneshot", "list", "open", "close", "refresh",
    "archive", "unarchive", "label", "remove", "merge", "send", "tab", "prune", "restore",
    "project", "completion",
];

const WORKTREE_COMMANDS: &[&str] = &[
    "add", "list", "open", "close", "refresh", "archive", "unarchive", "label", "remove", "merge",
    "send", "tab", "prune", "restore",
];

fn is_serve_root_option(value: &str) -> bool {
    matches!(
        value,
        "--port" | "--app" | "--debug" | "--help" | "-h" | "--version" | "-V"
    )
}

struct ParsedRoot {
    port: u16,
    port_explicit: bool,
    debug: bool,
    #[allow(dead_code)]
    app: bool,
    command: Option<String>,
    command_args: Vec<String>,
}

fn parse_root_args(args: &[String]) -> Result<ParsedRoot, String> {
    let mut port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let mut port_explicit = std::env::var("PORT").is_ok();
    let mut debug = false;
    let mut app = false;
    let mut command: Option<String> = None;
    let mut command_args: Vec<String> = Vec::new();

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg.is_empty() {
            index += 1;
            continue;
        }

        // Once a command is set, args flow to it (except serve's own options).
        if let Some(cmd) = &command {
            if cmd != "serve" || !is_serve_root_option(arg) {
                command_args.push(arg.clone());
                index += 1;
                continue;
            }
        }

        match arg.as_str() {
            "--port" => {
                let value = args.get(index + 1).ok_or("Error: --port requires a numeric value")?;
                port = value.parse().map_err(|_| "Error: --port requires a numeric value".to_string())?;
                port_explicit = true;
                index += 1;
            }
            "--app" => app = true,
            "--debug" => debug = true,
            "--version" | "-V" => {
                println!("{}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                usage();
                std::process::exit(0);
            }
            other => {
                if !ROOT_COMMANDS.contains(&other) {
                    return Err(format!("Unknown command or option: {other}\nRun sebenza-cli --help for usage."));
                }
                command = Some(other.to_string());
            }
        }
        index += 1;
    }

    Ok(ParsedRoot { port, port_explicit, debug, app, command, command_args })
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Internal: called by shell completion scripts.
    if args.first().map(String::as_str) == Some("--completions") {
        completions::handle_completions(&args[1..]);
        std::process::exit(0);
    }

    let parsed = match parse_root_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let Some(command) = parsed.command.clone() else {
        usage();
        std::process::exit(0);
    };

    let cwd = std::env::current_dir()
        .map(|c| c.to_string_lossy().to_string())
        .unwrap_or_default();

    if command == "serve" {
        std::process::exit(serve::run(parsed.port, parsed.app, &cwd).await);
    }
    if command == "completion" {
        std::process::exit(completions::run_completion_command(&parsed.command_args));
    }
    if command == "init" {
        std::process::exit(init::run(&cwd));
    }
    if command == "service" {
        std::process::exit(service::run_command(&parsed.command_args, &cwd));
    }
    if command == "update" {
        std::process::exit(update::run_update());
    }

    // When the user didn't pin a port, target the live server for this project
    // rather than the 5111 default.
    let effective_port = if parsed.port_explicit {
        parsed.port
    } else {
        let resolved = resolve_live_server_port(parsed.port, &cwd);
        if parsed.debug && resolved.source != port::PortSource::Default {
            eprintln!("[sebenza-cli] resolved port {} from live instance ({})", resolved.port, resolved.source.as_str());
        }
        resolved.port
    };

    // Nudge toward consolidation when other servers are running, on any command
    // that reaches a project — except `project migrate`, which consolidates them.
    let is_project_migrate =
        command == "project" && parsed.command_args.first().map(String::as_str) == Some("migrate");
    let reaches_project =
        command == "oneshot" || command == "project" || WORKTREE_COMMANDS.contains(&command.as_str());
    if reaches_project && !is_project_migrate {
        migrate::warn_if_other_instances(effective_port);
    }

    let code = if command == "oneshot" {
        oneshot::run(&parsed.command_args, effective_port, &cwd).await
    } else if command == "project" {
        project::run(&parsed.command_args, effective_port).await
    } else if WORKTREE_COMMANDS.contains(&command.as_str()) {
        worktree::run(worktree::WorktreeContext {
            command,
            args: parsed.command_args,
            project_dir: cwd,
            port: effective_port,
        })
        .await
    } else {
        usage();
        0
    };

    std::process::exit(code);
}
