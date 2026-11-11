//! `sebenza-cli init` — set up a project locally: check dependencies, scaffold
//! `.ai/sebenza.yaml`, and (when a coding agent is available) let it adapt
//! the starter config. Runs entirely locally; hits no server.

use std::path::Path;
use std::process::{Command, Stdio};

use common::services::init_authoring::{
    InitAgent, analyze_config, detect_init_project_context, scaffold_config,
};

fn optional_init_tools() -> Vec<&'static str> {
    let mut tools = vec!["gh", "claude", "grok", "codex", "goose", "opencode"];
    if cfg!(target_os = "linux") {
        tools.push("lxc-create");
    } else if cfg!(target_os = "macos") {
        tools.push("container");
    }
    tools
}

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

/// Resolve an optional tool, falling back to its own install directory when it is not on
/// `PATH`. opencode installs to `~/.opencode/bin` and grok to `~/.grok/bin`, neither a
/// conventional directory,
/// so a bare `which` reports a perfectly good install as missing — particularly for a
/// server started from systemd, which need not inherit the login shell's PATH.
/// Home-relative places a tool may live when it is not on `PATH`, keyed by tool name.
///
/// Separate from `resolve_optional_tool` so the table can be asserted without mutating
/// `HOME`/`PATH`, which would race the other tests in this binary.
fn home_relative_candidates(tool: &str) -> &'static [&'static str] {
    match tool {
        "opencode" => &[".opencode/bin/opencode"],
        // grok installs to ~/.grok/bin, which is not on a default PATH either.
        "grok" => &[".grok/bin/grok"],
        "goose" => &[".local/bin/goose"],
        _ => &[],
    }
}

fn resolve_optional_tool(tool: &str) -> Option<String> {
    if which(tool) {
        return Some(tool.to_string());
    }
    let home = std::env::var_os("HOME")?;
    home_relative_candidates(tool).iter().find_map(|rel| {
        let p = std::path::Path::new(&home).join(rel);
        p.is_file().then(|| p.to_string_lossy().to_string())
    })
}

/// Plugin id of the Sebenza skills: marketplace `sebenza` (this repository's
/// root) publishing plugin `sebenza` from `skills/`. Stable: other repositories'
/// docs point users at it.
const SEBENZA_PLUGIN_ID: &str = "sebenza@sebenza";
/// Where the skills were installed from before they moved into this repository.
const LEGACY_PLUGIN_ID: &str = "sebenza@gts-agent-skills";

#[derive(Debug, PartialEq, Eq)]
enum PluginStatus {
    Installed { version: Option<String> },
    Legacy,
    Missing,
}

/// Classify Claude Code's `installed_plugins.json`. Pure, so it is testable
/// without a Claude install; a missing or unreadable file means not installed.
fn sebenza_plugin_status(installed_plugins_json: Option<&str>) -> PluginStatus {
    let Some(doc) = installed_plugins_json
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
    else {
        return PluginStatus::Missing;
    };
    let plugins = &doc["plugins"];
    if let Some(entries) = plugins.get(SEBENZA_PLUGIN_ID) {
        let version = entries
            .as_array()
            .and_then(|list| list.iter().find_map(|e| e["version"].as_str()))
            .map(str::to_string);
        return PluginStatus::Installed { version };
    }
    if plugins.get(LEGACY_PLUGIN_ID).is_some() {
        return PluginStatus::Legacy;
    }
    PluginStatus::Missing
}

fn plugin_status_line(status: &PluginStatus) -> String {
    let install = format!(
        "claude plugin marketplace add groundtruthsystems/sebenza && claude plugin install {SEBENZA_PLUGIN_ID}"
    );
    match status {
        PluginStatus::Installed { version: Some(v) } => {
            format!("  ✓ Sebenza skills plugin ({SEBENZA_PLUGIN_ID} {v})")
        }
        PluginStatus::Installed { version: None } => {
            format!("  ✓ Sebenza skills plugin ({SEBENZA_PLUGIN_ID})")
        }
        PluginStatus::Legacy => format!(
            "  ○ Sebenza skills plugin — installed from the old marketplace ({LEGACY_PLUGIN_ID}); \
             the skills now ship from this repository. Switch with: {install}"
        ),
        PluginStatus::Missing => {
            format!("  ○ Sebenza skills plugin — not installed (optional). Install with: {install}")
        }
    }
}

fn report_sebenza_plugin() {
    let text = std::env::var_os("HOME").and_then(|home| {
        std::fs::read_to_string(Path::new(&home).join(".claude/plugins/installed_plugins.json"))
            .ok()
    });
    println!(
        "{}",
        plugin_status_line(&sebenza_plugin_status(text.as_deref()))
    );
}

/// Best-effort `--version`. Reported so a user can see WHICH version they have when a
/// session-format or hook change breaks history, rather than guessing.
fn tool_version(path: &str) -> Option<String> {
    let out = std::process::Command::new(path)
        .arg("--version")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(60).collect())
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
    let optional = optional_init_tools();
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
        if let Some(path) = resolve_optional_tool(tool) {
            match tool_version(&path) {
                Some(v) => println!("  ✓ {tool} ({v})"),
                None => println!("  ✓ {tool}"),
            }
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

    // Step 3b — the Sebenza skills plugin for Claude Code (informational).
    if which("claude") {
        report_sebenza_plugin();
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
    let agent = if codex && !claude {
        InitAgent::Codex
    } else {
        InitAgent::Claude
    };
    let has_agent = claude || codex;

    let ctx = detect_init_project_context(&root, agent);

    if let Err(e) = scaffold_config(&ctx) {
        eprintln!("Failed to create .ai/sebenza.yaml: {e}");
        eprintln!("Setup incomplete.");
        return 1;
    }
    println!(".ai/sebenza.yaml starter template created");

    if has_agent {
        let label = if matches!(agent, InitAgent::Codex) {
            "Codex"
        } else {
            "Claude"
        };
        println!("Running {label} to adapt the starter .ai/sebenza.yaml...");
        let unique = std::process::id().to_string();
        match analyze_config(&ctx, &unique) {
            Ok(()) => println!("{label} adapted .ai/sebenza.yaml"),
            Err(e) => {
                println!("{label} could not adapt the config ({e}) — keeping the starter template")
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built-in agent must be detected, or `init` reports a working install as
    /// missing and the user has no way to tell why their agent never starts.
    #[test]
    fn every_builtin_agent_cli_is_detected() {
        let tools = optional_init_tools();
        for agent in ["claude", "grok", "codex", "opencode"] {
            assert!(tools.contains(&agent), "{agent} is a built-in agent");
        }
    }

    /// grok installs to `~/.grok/bin` and opencode to `~/.opencode/bin`, neither on a
    /// default `PATH` - so a bare `which` reports a good install as missing, particularly
    /// for a server started from systemd, which need not inherit the login shell's PATH.
    #[test]
    fn the_agents_with_unconventional_install_dirs_have_home_relative_fallbacks() {
        assert_eq!(home_relative_candidates("grok"), &[".grok/bin/grok"]);
        assert_eq!(
            home_relative_candidates("opencode"),
            &[".opencode/bin/opencode"]
        );
        // claude and codex install onto PATH, so they need no fallback.
        assert!(home_relative_candidates("claude").is_empty());
        assert!(home_relative_candidates("codex").is_empty());
    }

    /// The skills ship from this repository as `sebenza@sebenza` (track
    /// software_factory_20261111, AC-M.0); `init` must say which one is installed.
    #[test]
    fn plugin_status_finds_the_sebenza_marketplace_install() {
        let json =
            r#"{"version":2,"plugins":{"sebenza@sebenza":[{"scope":"user","version":"1.1.0"}]}}"#;
        assert_eq!(
            sebenza_plugin_status(Some(json)),
            PluginStatus::Installed {
                version: Some("1.1.0".into())
            }
        );
    }

    #[test]
    fn plugin_status_flags_the_old_gts_marketplace_install() {
        let json = r#"{"version":2,"plugins":{"sebenza@gts-agent-skills":[{"scope":"local"}]}}"#;
        assert_eq!(sebenza_plugin_status(Some(json)), PluginStatus::Legacy);
    }

    #[test]
    fn plugin_status_is_missing_without_a_file_an_entry_or_valid_json() {
        assert_eq!(sebenza_plugin_status(None), PluginStatus::Missing);
        assert_eq!(
            sebenza_plugin_status(Some("not json")),
            PluginStatus::Missing
        );
        let other = r#"{"version":2,"plugins":{"kaituhi@gts-agent-skills":[{}]}}"#;
        assert_eq!(sebenza_plugin_status(Some(other)), PluginStatus::Missing);
    }

    #[test]
    fn plugin_status_lines_name_the_stable_install_commands() {
        let missing = plugin_status_line(&PluginStatus::Missing);
        assert!(
            missing.contains("claude plugin install sebenza@sebenza"),
            "{missing}"
        );
        let legacy = plugin_status_line(&PluginStatus::Legacy);
        assert!(legacy.contains("sebenza@gts-agent-skills"), "{legacy}");
        assert!(legacy.contains("sebenza@sebenza"), "{legacy}");
    }

    #[test]
    fn optional_tools_include_the_platform_sandbox_cli_not_docker() {
        let tools = optional_init_tools();
        assert!(!tools.contains(&"docker"));
        assert!(tools.contains(&"gh"));
        if cfg!(target_os = "linux") {
            assert!(tools.contains(&"lxc-create"));
            assert!(!tools.contains(&"container"));
        } else if cfg!(target_os = "macos") {
            assert!(tools.contains(&"container"));
            assert!(!tools.contains(&"lxc-create"));
        }
    }
}
