//! `sebenza-cli service` — manage Sebenza as a per-machine system service
//! (systemd on Linux, launchd on macOS). One service serves every registered
//! project on one port; add more projects with `sebenza-cli project add`.

use std::io::IsTerminal;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use common::adapters::projects_registry::{ProjectEntry, ProjectsRegistry};
use common::config::project_root;
use common::domain::policies::is_valid_env_key;

use crate::service_units::{
    detect_platform, generate_service_file, read_port_from_unit, resolve_server_path, unit_path,
    Platform, ServiceConfig, RESERVED_ENV_KEYS, SERVICE_NAME,
};

const DEFAULT_PORT: u16 = 5111;

const USAGE: &str = "
sebenza-cli service — Manage Sebenza as a system service

Sebenza runs as a single multi-project service per machine. Install it
once; add more projects from the dashboard or with `sebenza-cli project add`.

Usage:
  sebenza-cli service install     Install, enable, and start the service
  sebenza-cli service uninstall   Stop, disable, and remove the service
  sebenza-cli service status      Show service status
  sebenza-cli service logs        Tail service logs

Options:
  --port N                   Pin the service to a port (default: 5111). On
                             reinstall without --port the existing port is kept.
  --yes, -y                  Skip the confirmation prompt and install. In a
                             non-interactive shell (CI, pipe) install prints the
                             plan and stops unless --yes is passed.
  --env KEY=VALUE            Bake an environment variable into the service
                             unit (repeatable). Reserved keys PORT,
                             SEBENZA_PROJECT_DIR, and PATH are rejected.
  --no-auto-env              Skip auto-detection of env vars (default: detect).

  When any env var is set, the unit file is written with mode 0600 so
  secrets are readable only by the installing user.";

struct Args {
    port: u16,
    port_explicit: bool,
    auto_confirm: bool,
    env: Vec<(String, String)>,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut port = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
    let mut port_explicit = false;
    let mut auto_confirm = false;
    let mut env: Vec<(String, String)> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--port" => {
                let v = args.get(i + 1).ok_or("--port requires a numeric value")?;
                port = v.parse().map_err(|_| "--port requires a numeric value".to_string())?;
                port_explicit = true;
                i += 1;
            }
            "--yes" | "-y" => auto_confirm = true,
            "--no-auto-env" => {}
            "--env" => {
                let raw = args.get(i + 1).ok_or("--env requires a KEY=VALUE argument")?;
                let (k, v) = parse_env(raw)?;
                env.push((k, v));
                i += 1;
            }
            other if other.starts_with("--env=") => {
                let (k, v) = parse_env(&other["--env=".len()..])?;
                env.push((k, v));
            }
            other => return Err(format!("Unknown option: {other}")),
        }
        i += 1;
    }
    Ok(Args { port, port_explicit, auto_confirm, env })
}

fn parse_env(raw: &str) -> Result<(String, String), String> {
    let eq = raw.find('=').unwrap_or(0);
    if eq == 0 {
        return Err(format!("--env expects KEY=VALUE (got: {raw})"));
    }
    let key = raw[..eq].to_string();
    if !is_valid_env_key(&key) {
        return Err(format!("--env key is not a valid identifier: {key}"));
    }
    if RESERVED_ENV_KEYS.contains(&key.as_str()) {
        return Err(format!("--env cannot set {key} — it is managed by the service unit"));
    }
    Ok((key, raw[eq + 1..].to_string()))
}

fn manager_bin(platform: Platform) -> &'static str {
    match platform {
        Platform::Linux => "systemctl",
        Platform::Macos => "launchctl",
    }
}

fn which(bin: &str) -> bool {
    Command::new("which").arg(bin).stdout(Stdio::null()).stderr(Stdio::null())
        .status().map(|s| s.success()).unwrap_or(false)
}

/// Run a command, capturing output; returns (ok, stderr).
fn run(bin: &str, args: &[&str]) -> (bool, String) {
    match Command::new(bin).args(args).output() {
        Ok(out) => (out.status.success(), String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => (false, e.to_string()),
    }
}

pub fn run_command(args: &[String], cwd: &str) -> i32 {
    let action = match args.first().map(String::as_str) {
        None | Some("--help") | Some("-h") => {
            println!("{USAGE}");
            return 0;
        }
        Some(a) => a.to_string(),
    };
    if !matches!(action.as_str(), "install" | "uninstall" | "status" | "logs") {
        eprintln!("Unknown action: {action}");
        println!("{USAGE}");
        return 1;
    }

    let Some(platform) = detect_platform() else {
        eprintln!("Unsupported platform. Only linux and macOS are supported.");
        return 1;
    };
    let mgr = manager_bin(platform);
    if !which(mgr) {
        eprintln!("{mgr} not found. Cannot manage services on this system.");
        return 1;
    }

    match action.as_str() {
        "install" => {
            let parsed = match parse_args(&args[1..]) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{e}");
                    return 1;
                }
            };
            install(platform, mgr, parsed, cwd)
        }
        "uninstall" => uninstall(platform, mgr),
        "status" => status(platform, mgr),
        "logs" => logs(platform),
        _ => unreachable!(),
    }
}

fn confirm(message: &str) -> bool {
    use std::io::Write;
    print!("{message} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

fn should_persist_project(cwd: &str) -> Option<(String, String)> {
    let root = project_root(cwd);
    let has_config = std::path::Path::new(&root).join(".ai/sebenza.yaml").exists()
        || std::path::Path::new(&root).join(".ai/sebenza.local.yaml").exists();
    if !has_config {
        return None;
    }
    let registry = ProjectsRegistry::new();
    if registry.list().iter().any(|e| e.path == root) {
        return None;
    }
    let name = std::path::Path::new(&root)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| root.clone());
    Some((root, name))
}

fn install(platform: Platform, mgr: &str, args: Args, cwd: &str) -> i32 {
    let file = unit_path(platform);
    let already = file.is_file();

    // Port: explicit wins; else reuse existing unit's port; else default.
    let mut port = args.port;
    let mut port_note: Option<String> = None;
    if !args.port_explicit && already {
        if let Ok(content) = std::fs::read_to_string(&file) {
            if let Some(existing) = read_port_from_unit(&content) {
                port = existing;
                port_note = Some(format!(
                    "Reusing port {existing} from the existing service unit (pass --port to override)."
                ));
            }
        }
    }

    let config = ServiceConfig {
        platform,
        server_path: resolve_server_path(),
        port,
        env: args.env.clone(),
    };
    let content = generate_service_file(&config);
    let persist = should_persist_project(cwd);

    // Plan.
    println!("Install service");
    if already {
        println!("Service is already installed — this will reinstall it.");
    }
    println!("  File: {}", file.display());
    if let Some(note) = &port_note {
        println!("  {note}");
    }
    if !args.env.is_empty() {
        let redacted: Vec<String> = args
            .env
            .iter()
            .map(|(k, v)| {
                let ku = k.to_uppercase();
                if ["TOKEN", "KEY", "PASSWORD", "SECRET"].iter().any(|s| ku.ends_with(s)) {
                    format!("    {k}=••• ({} chars)", v.len())
                } else {
                    format!("    {k}={v}")
                }
            })
            .collect();
        println!("  Environment variables baked into the unit:\n{}", redacted.join("\n"));
    }
    if let Some((path, name)) = &persist {
        println!("  Will also register this project: {name} ({path})");
    }

    // Confirm gate.
    let interactive = std::io::stdin().is_terminal();
    if !args.auto_confirm {
        if interactive {
            let msg = if already { "Reinstall?" } else { "Proceed?" };
            if !confirm(msg) {
                println!("Aborted.");
                return 0;
            }
        } else {
            let verb = if already { "reinstalling" } else { "installing" };
            println!(
                "Non-interactive environment — not {verb}. Re-run with --yes to confirm and apply the plan above."
            );
            return 0;
        }
    }

    // Reinstall: tear down the old unit first.
    if already {
        for cmd in uninstall_commands(platform) {
            let _ = run(&cmd[0], &cmd[1..].iter().map(String::as_str).collect::<Vec<_>>());
        }
    }

    if let Some(parent) = file.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("Failed to create {}: {e}", parent.display());
            return 1;
        }
    }
    if let Err(e) = std::fs::write(&file, &content) {
        eprintln!("Failed to write {}: {e}", file.display());
        return 1;
    }
    if !args.env.is_empty() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)) {
                println!("Wrote {} but could not chmod 600: {e}", file.display());
            }
        }
    }
    println!("Wrote {}", file.display());

    // Register the project.
    if let Some((path, name)) = persist {
        let added_at = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        ProjectsRegistry::new().add(ProjectEntry { path: path.clone(), name: name.clone(), added_at });
        println!("Registered project {name} ({path})");
    }

    // Enable + start.
    for cmd in install_commands(platform) {
        let refs: Vec<&str> = cmd[1..].iter().map(String::as_str).collect();
        let (ok, stderr) = run(&cmd[0], &refs);
        if !ok {
            eprintln!("Command failed: {}\n{stderr}", cmd.join(" "));
            return 1;
        }
        println!("$ {}", cmd.join(" "));
    }
    println!("Service installed and started!");
    if platform == Platform::Linux {
        println!("Tip: run `loginctl enable-linger $USER` so it keeps running after you log out.");
    }
    println!("Check status: sebenza-cli service status");
    println!("View logs:    sebenza-cli service logs");
    let _ = mgr;
    0
}

fn install_commands(platform: Platform) -> Vec<Vec<String>> {
    match platform {
        Platform::Linux => vec![
            vec!["systemctl".into(), "--user".into(), "daemon-reload".into()],
            vec!["systemctl".into(), "--user".into(), "enable".into(), "--now".into(), SERVICE_NAME.into()],
        ],
        Platform::Macos => {
            vec![vec!["launchctl".into(), "load".into(), "-w".into(), unit_path(platform).to_string_lossy().to_string()]]
        }
    }
}

fn uninstall_commands(platform: Platform) -> Vec<Vec<String>> {
    match platform {
        Platform::Linux => vec![
            vec!["systemctl".into(), "--user".into(), "stop".into(), SERVICE_NAME.into()],
            vec!["systemctl".into(), "--user".into(), "disable".into(), SERVICE_NAME.into()],
        ],
        Platform::Macos => {
            vec![vec!["launchctl".into(), "unload".into(), "-w".into(), unit_path(platform).to_string_lossy().to_string()]]
        }
    }
}

fn uninstall(platform: Platform, _mgr: &str) -> i32 {
    let file = unit_path(platform);
    if !file.is_file() {
        eprintln!("Service is not installed.");
        return 1;
    }
    println!("Uninstall service");
    println!("  File to remove: {}", file.display());
    if !confirm("Proceed?") {
        println!("Aborted.");
        return 0;
    }
    for cmd in uninstall_commands(platform) {
        let refs: Vec<&str> = cmd[1..].iter().map(String::as_str).collect();
        let (ok, stderr) = run(&cmd[0], &refs);
        if !ok {
            eprintln!("Warning: {} failed: {stderr}", cmd.join(" "));
        }
    }
    if let Err(e) = std::fs::remove_file(&file) {
        eprintln!("Could not remove {}: {e}", file.display());
        return 1;
    }
    println!("Removed {}", file.display());
    println!("Service uninstalled.");
    0
}

fn status(platform: Platform, _mgr: &str) -> i32 {
    let file = unit_path(platform);
    if !file.is_file() {
        eprintln!("Service is not installed.");
        return 1;
    }
    let mut cmd = match platform {
        Platform::Linux => {
            let mut c = Command::new("systemctl");
            c.args(["--user", "status", SERVICE_NAME]);
            c
        }
        Platform::Macos => {
            let mut c = Command::new("launchctl");
            c.args(["list", &format!("com.sebenza.{SERVICE_NAME}")]);
            c
        }
    };
    cmd.status().map(|s| s.code().unwrap_or(0)).unwrap_or(1)
}

fn logs(platform: Platform) -> i32 {
    let file = unit_path(platform);
    if !file.is_file() {
        eprintln!("Service is not installed.");
        return 1;
    }
    match platform {
        Platform::Linux => Command::new("journalctl")
            .args(["--user", "-u", SERVICE_NAME, "-f", "--no-pager"])
            .status()
            .map(|s| s.code().unwrap_or(0))
            .unwrap_or(1),
        Platform::Macos => {
            let log = crate::service_units::launchd_log_path();
            if !log.is_file() {
                eprintln!("Log file not found: {}", log.display());
                return 1;
            }
            Command::new("tail")
                .arg("-f")
                .arg(&log)
                .status()
                .map(|s| s.code().unwrap_or(0))
                .unwrap_or(1)
        }
    }
}
