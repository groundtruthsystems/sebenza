//! Shared system-service plumbing: unit-file paths, templates, and discovery.
//! `sebenza` runs as a single machine-wide service (`sebenza`) that serves every
//! registered project on one port. Used by `service`, `update`, and `migrate`.

use std::path::PathBuf;

/// Fixed service name — one multi-project service per machine.
pub const SERVICE_NAME: &str = "sebenza";

/// Env keys the unit manages itself; users may not override them via `--env`.
pub const RESERVED_ENV_KEYS: &[&str] = &["PORT", "SEBENZA_PROJECT_DIR", "PATH"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Macos,
}

pub fn detect_platform() -> Option<Platform> {
    if cfg!(target_os = "linux") {
        Some(Platform::Linux)
    } else if cfg!(target_os = "macos") {
        Some(Platform::Macos)
    } else {
        None
    }
}

pub fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/root".to_string())
}

pub fn systemd_dir() -> PathBuf {
    PathBuf::from(home())
        .join(".config")
        .join("systemd")
        .join("user")
}

pub fn launchd_dir() -> PathBuf {
    PathBuf::from(home()).join("Library").join("LaunchAgents")
}

pub fn unit_path(platform: Platform) -> PathBuf {
    match platform {
        Platform::Linux => systemd_dir().join(format!("{SERVICE_NAME}.service")),
        Platform::Macos => launchd_dir().join(format!("com.sebenza.{SERVICE_NAME}.plist")),
    }
}

pub fn launchd_log_path() -> PathBuf {
    PathBuf::from(home())
        .join("Library")
        .join("Logs")
        .join(format!("{SERVICE_NAME}.log"))
}

/// A discovered installed service unit.
pub struct InstalledService {
    pub name: String,
    pub file_path: PathBuf,
    pub platform: Platform,
}

pub struct ServiceConfig {
    pub platform: Platform,
    /// Absolute path to the `sebenza-server` binary the unit should run.
    pub server_path: String,
    pub port: u16,
    /// Extra environment variables baked into the unit (key → value).
    pub env: Vec<(String, String)>,
}

fn path_env() -> String {
    std::env::var("PATH").unwrap_or_default()
}

pub fn generate_service_file(config: &ServiceConfig) -> String {
    match config.platform {
        Platform::Linux => generate_systemd_unit(config),
        Platform::Macos => generate_launchd_plist(config),
    }
}

fn generate_systemd_unit(config: &ServiceConfig) -> String {
    let mut env_lines = String::new();
    let mut env = config.env.clone();
    env.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in env {
        env_lines.push_str(&format!("Environment={k}={v}\n"));
    }
    format!(
        "[Unit]\n\
         Description=Sebenza dashboard\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={server} serve --port {port}\n\
         WorkingDirectory={home}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         Environment=PORT={port}\n\
         Environment=PATH={path}\n\
         {env_lines}\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        server = config.server_path,
        port = config.port,
        home = home(),
        path = path_env(),
    )
}

fn escape_plist(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn generate_launchd_plist(config: &ServiceConfig) -> String {
    let mut env_entries = String::new();
    let mut env = config.env.clone();
    env.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in env {
        env_entries.push_str(&format!(
            "    <key>{}</key><string>{}</string>\n",
            escape_plist(&k),
            escape_plist(&v)
        ));
    }
    let log = launchd_log_path();
    let log = log.to_string_lossy();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20 <key>Label</key>\n\
         \x20 <string>com.sebenza.{name}</string>\n\
         \x20 <key>ProgramArguments</key>\n\
         \x20 <array>\n\
         \x20   <string>{server}</string>\n\
         \x20   <string>serve</string>\n\
         \x20   <string>--port</string>\n\
         \x20   <string>{port}</string>\n\
         \x20 </array>\n\
         \x20 <key>WorkingDirectory</key>\n\
         \x20 <string>{home}</string>\n\
         \x20 <key>RunAtLoad</key>\n\
         \x20 <true/>\n\
         \x20 <key>KeepAlive</key>\n\
         \x20 <dict>\n\
         \x20   <key>SuccessfulExit</key>\n\
         \x20   <false/>\n\
         \x20 </dict>\n\
         \x20 <key>StandardOutPath</key>\n\
         \x20 <string>{log}</string>\n\
         \x20 <key>StandardErrorPath</key>\n\
         \x20 <string>{log}</string>\n\
         \x20 <key>EnvironmentVariables</key>\n\
         \x20 <dict>\n\
         \x20   <key>PORT</key>\n\
         \x20   <string>{port}</string>\n\
         \x20   <key>PATH</key>\n\
         \x20   <string>{path}</string>\n\
         {env_entries}\
         \x20 </dict>\n\
         </dict>\n\
         </plist>\n",
        name = SERVICE_NAME,
        server = config.server_path,
        port = config.port,
        home = home(),
        path = escape_plist(&path_env()),
    )
}

/// Extract the `--port` value baked into an existing unit file, if any.
pub fn read_port_from_unit(content: &str) -> Option<u16> {
    // systemd: `... serve --port 5111`; plist: `<string>--port</string><string>5111</string>`.
    let mut tokens = content.split_whitespace().peekable();
    while let Some(tok) = tokens.next() {
        let cleaned = tok.trim_matches(|c| c == '<' || c == '>' || c == '"');
        if cleaned == "--port" || cleaned.ends_with(">--port") {
            if let Some(next) = tokens.next() {
                let val: String = next.chars().filter(|c| c.is_ascii_digit()).collect();
                if let Ok(p) = val.parse::<u16>() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Enumerate installed `sebenza` service units across systemd + launchd.
pub fn list_installed_services() -> Vec<InstalledService> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(systemd_dir()) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if (name == "sebenza.service"
                || (name.starts_with("sebenza-") && name.ends_with(".service")))
                && entry.path().is_file()
            {
                out.push(InstalledService {
                    name: name.trim_end_matches(".service").to_string(),
                    file_path: entry.path(),
                    platform: Platform::Linux,
                });
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(launchd_dir()) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("com.sebenza.") && name.ends_with(".plist") {
                out.push(InstalledService {
                    name: name
                        .trim_start_matches("com.sebenza.")
                        .trim_end_matches(".plist")
                        .to_string(),
                    file_path: entry.path(),
                    platform: Platform::Macos,
                });
            }
        }
    }
    out
}

/// Resolve the absolute path to the `sebenza-server` binary for ExecStart.
pub fn resolve_server_path() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("sebenza-server");
            if sibling.is_file() {
                return sibling.to_string_lossy().to_string();
            }
        }
    }
    if let Ok(out) = std::process::Command::new("which")
        .arg("sebenza-server")
        .output()
    {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return p;
            }
        }
    }
    "sebenza-server".to_string()
}
