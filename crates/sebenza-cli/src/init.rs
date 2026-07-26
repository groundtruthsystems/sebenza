//! `sebenza-cli init` — set up a project locally: check dependencies, scaffold
//! `.ai/sebenza.yaml`, and (when a coding agent is available) let it adapt
//! the starter config. Runs entirely locally; hits no server.

use std::path::Path;
use std::process::{Command, Stdio};

use common::services::init_authoring::{
    analyze_config, detect_init_project_context, scaffold_config, InitAgent,
};

fn which(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn git_root(cwd: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!root.is_empty()).then_some(root)
}

pub fn run(cwd: &str) -> i32 {
    println!("sebenza-cli init");

    // Step 1 — git root.
    let Some(root) = git_root(cwd) else {
        eprintln!("Not inside a git repository. Run this from within a project.");
        eprintln!("Aborted.");
        return 1;
    };
    println!("Git root: {root}");

    // Step 2 — dependency checks.
    println!("Checking dependencies...");
    let required: [(&str, &str); 2] = [
        ("git", "https://git-scm.com"),
        ("tmux", "https://github.com/tmux/tmux/wiki/Installing"),
    ];
    let optional = ["gh", "claude", "codex", "goose", "opencode", "docker"];
    let mut missing_required: Vec<(&str, &str)> = Vec::new();
    for (tool, hint) in required {
        if which(tool) {
            println!("  ✓ {tool}");
        } else {
            println!("  ✗ {tool} — not found (required)");
            missing_required.push((tool, hint));
        }
    }
    for tool in optional {
        if which(tool) {
            println!("  ✓ {tool}");
        } else {
            println!("  ○ {tool} — not found (optional)");
        }
    }
    if !missing_required.is_empty() {
        println!("\nInstall these required dependencies, then re-run sebenza-cli init:");
        for (tool, hint) in &missing_required {
            println!("  {tool}: {hint}");
        }
        eprintln!("Setup incomplete.");
        return 1;
    }

    // Step 3 — gh auth (informational).
    if which("gh") {
        let authed = Command::new("gh")
            .args(["auth", "status"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if authed {
            println!("  ✓ gh — authenticated");
        } else {
            println!("  gh is installed but not authenticated. Run: gh auth login");
        }
    }

    // Step 4 — config.
    println!("Checking config files...");
    let config_path = Path::new(&root).join(".ai").join("sebenza.yaml");
    if config_path.exists() {
        println!(".ai/sebenza.yaml already exists, skipping");
        finish();
        return 0;
    }

    let claude = which("claude");
    let codex = which("codex");
    // Mirror `authoring_agent`: codex only when it's present and claude isn't.
    let agent = if codex && !claude { InitAgent::Codex } else { InitAgent::Claude };
    let has_agent = claude || codex;

    let ctx = detect_init_project_context(&root, agent);

    if let Err(e) = scaffold_config(&ctx) {
        eprintln!("Failed to create .ai/sebenza.yaml: {e}");
        eprintln!("Setup incomplete.");
        return 1;
    }
    println!(".ai/sebenza.yaml starter template created");

    if has_agent {
        let label = if matches!(agent, InitAgent::Codex) { "Codex" } else { "Claude" };
        println!("Running {label} to adapt the starter .ai/sebenza.yaml...");
        let unique = std::process::id().to_string();
        match analyze_config(&ctx, &unique) {
            Ok(()) => println!("{label} adapted .ai/sebenza.yaml"),
            Err(e) => println!("{label} could not adapt the config ({e}) — keeping the starter template"),
        }
    }

    finish();
    0
}

fn finish() {
    println!("\nYou're all set! Next steps:");
    println!("  1. Review .ai/sebenza.yaml and adjust panes, ports, and profiles if needed");
    println!("  2. Run: sebenza-cli serve");
    println!("  3. Enable tab completion: eval \"$(sebenza-cli completion zsh)\"  (or bash)");
}
