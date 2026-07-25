//! Project subcommands: `ls`, `add`, `rm`, `migrate`.

use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};

use crate::http::Http;

const POLL_INTERVAL: Duration = Duration::from_millis(700);
const SETUP_TIMEOUT: Duration = Duration::from_secs(5 * 60);

enum ProjectCommand {
    Ls,
    Add(String),
    Rm(String),
    Migrate,
}

fn usage() -> String {
    [
        "Usage:",
        "  sebenza-cli project ls                 List projects the dashboard is serving",
        "  sebenza-cli project add [path]         Add a project (defaults to the current repo)",
        "  sebenza-cli project rm <prefix>        Remove a project by its prefix",
        "  sebenza-cli project migrate            Fold other running Sebenza servers into this one",
        "",
        "All projects are served together on one dashboard and one port. `add` persists",
        "the project so it is reloaded on the next start. These commands talk to the live",
        "Sebenza server for this directory (or the server on --port when given).",
        "",
        "Examples:",
        "  sebenza-cli project ls",
        "  sebenza-cli project add ~/code/my-service",
        "  sebenza-cli project rm my-service",
        "  sebenza-cli project migrate",
    ]
    .join("\n")
}

fn parse(args: &[String]) -> Result<Option<ProjectCommand>> {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        return Ok(None);
    }
    let sub = args[0].as_str();
    match sub {
        "ls" | "list" => {
            if args.len() > 1 {
                return Err(anyhow!("Unexpected argument: {}", args[1]));
            }
            Ok(Some(ProjectCommand::Ls))
        }
        "add" => {
            if args.len() > 2 {
                return Err(anyhow!("Unexpected argument: {}", args[2]));
            }
            Ok(Some(ProjectCommand::Add(args.get(1).cloned().unwrap_or_else(|| ".".to_string()))))
        }
        "rm" | "remove" => {
            let prefix = args.get(1).ok_or_else(|| anyhow!("project rm requires a <prefix> argument"))?;
            if args.len() > 2 {
                return Err(anyhow!("Unexpected argument: {}", args[2]));
            }
            Ok(Some(ProjectCommand::Rm(prefix.clone())))
        }
        "migrate" => {
            if args.len() > 1 {
                return Err(anyhow!("Unexpected argument: {}", args[1]));
            }
            Ok(Some(ProjectCommand::Migrate))
        }
        other => Err(anyhow!("Unknown project subcommand: {other}")),
    }
}

fn phase_label(phase: &str) -> Option<String> {
    match phase {
        "creating_config" => Some("Creating .ai/sebenza.yaml".to_string()),
        "analyzing" => Some("Analyzing project structure".to_string()),
        _ => None,
    }
}

pub async fn run(args: &[String], port: u16) -> i32 {
    let parsed = match parse(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            eprintln!("{}", usage());
            return 1;
        }
    };
    let Some(cmd) = parsed else {
        println!("{}", usage());
        return 0;
    };

    if matches!(&cmd, ProjectCommand::Migrate) {
        return crate::migrate::run_migrate(port).await;
    }

    match run_inner(cmd, port).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

async fn run_inner(cmd: ProjectCommand, port: u16) -> Result<i32> {
    let http = Http::new(port);
    match cmd {
        // `migrate` is handled before `run_inner` (see `run`).
        ProjectCommand::Migrate => unreachable!(),
        ProjectCommand::Ls => {
            let projects = http.fetch_projects().await?;
            if projects.is_empty() {
                println!("No projects. Add one with: sebenza-cli project add [path]");
                return Ok(0);
            }
            for p in projects {
                let marker = if p.active { "●" } else { "○" };
                println!("{marker} {}\t{}\t{}", p.prefix, p.name, p.path);
            }
            Ok(0)
        }
        ProjectCommand::Add(path) => {
            let absolute = std::fs::canonicalize(&path)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| {
                    let p = std::path::Path::new(&path);
                    if p.is_absolute() {
                        path.clone()
                    } else {
                        std::env::current_dir()
                            .map(|c| c.join(p).to_string_lossy().to_string())
                            .unwrap_or_else(|_| path.clone())
                    }
                });
            let res = http.add_project(&absolute).await?;
            if !res.initializing {
                let project = res
                    .project
                    .ok_or_else(|| anyhow!("Server accepted the project but returned nothing to open."))?;
                println!("Added {} ({}) — {}", project.name, project.prefix, project.path);
                return Ok(0);
            }
            // Repo had no .ai/sebenza.yaml — the server is setting it up. Follow the phases.
            let ready = await_setup(&http, &res.path).await?;
            let name = ready.name.clone().unwrap_or_else(|| ready.prefix.clone().unwrap_or_default());
            let prefix = ready.prefix.clone().unwrap_or_default();
            println!("Added {name} ({prefix}) — {}", res.path);
            Ok(0)
        }
        ProjectCommand::Rm(prefix) => {
            http.remove_project(&prefix).await?;
            println!("Removed project: {prefix}");
            Ok(0)
        }
    }
}

/// Follow an in-progress on-add project setup to completion, printing each
/// phase as it changes. Returns the ready init state or errors on failure/timeout.
async fn await_setup(http: &Http, path: &str) -> Result<crate::http::ProjectInit> {
    let deadline = Instant::now() + SETUP_TIMEOUT;
    let mut last_phase: Option<String> = None;

    while Instant::now() < deadline {
        // A transient poll failure shouldn't abort the flow — the backend job
        // keeps running, so just retry until the deadline.
        let state = http
            .project_inits()
            .await
            .ok()
            .and_then(|inits| inits.into_iter().find(|i| i.path == path));

        if let Some(state) = state {
            if last_phase.as_deref() != Some(state.phase.as_str()) {
                last_phase = Some(state.phase.clone());
                if state.phase != "ready" && state.phase != "failed" {
                    if let Some(label) = phase_label(&state.phase) {
                        println!("  {label}…");
                    }
                }
            }
            if state.phase == "ready" {
                return Ok(state);
            }
            if state.phase == "failed" {
                return Err(anyhow!(state.error.unwrap_or_else(|| "Project setup failed.".to_string())));
            }
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    Err(anyhow!("Project setup timed out."))
}
