//! `sebenza-cli serve` — start the dashboard by spawning the `sebenza-server` daemon.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tokio::process::Command as TokioCommand;

use crate::env_files::load_project_env;

/// Locate the `sebenza-server` binary: prefer one next to this executable, else
/// fall back to `PATH`.
fn find_server_binary() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("sebenza-server");
            if sibling.is_file() {
                return sibling.to_string_lossy().to_string();
            }
        }
    }
    "sebenza-server".to_string()
}

/// Locate the built SPA assets to hand the server via `$SEBENZA_FRONTEND_DIST`.
/// Searches near the executable (covers both an installed layout and the dev
/// `target/<profile>/sebenza-cli` → repo `frontend/dist`). Returns None if not found;
/// the server then runs API-only.
fn find_frontend_dist() -> Option<PathBuf> {
    if std::env::var_os("SEBENZA_FRONTEND_DIST").is_some() {
        return None; // caller already set it; leave as-is
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    for rel in [
        "frontend/dist",
        "../frontend/dist",
        "../../frontend/dist",
        "../share/sebenza-cli/frontend/dist",
    ] {
        let candidate = dir.join(rel);
        if candidate.is_dir() {
            return std::fs::canonicalize(&candidate).ok().or(Some(candidate));
        }
    }
    None
}

fn find_browser() -> Option<String> {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        ]
    } else {
        &[
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "microsoft-edge",
            "brave-browser",
        ]
    };
    for cand in candidates {
        if cand.starts_with('/') {
            if Path::new(cand).exists() {
                return Some(cand.to_string());
            }
        } else if Command::new("which")
            .arg(cand)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Some(cand.to_string());
        }
    }
    None
}

fn open_app_mode(url: &str) {
    match find_browser() {
        None => println!("[app] No Chromium-based browser found — open {url} manually"),
        Some(browser) => {
            println!("[app] Opening {url} in app mode");
            let _ = Command::new(browser)
                .arg(format!("--app={url}"))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }
}

pub async fn run(port: u16, app: bool, cwd: &str) -> i32 {
    let keys = load_project_env(cwd);

    if let Some(dist) = find_frontend_dist() {
        // SAFETY: single-threaded CLI startup, before other tasks run.
        unsafe { std::env::set_var("SEBENZA_FRONTEND_DIST", dist) };
    }

    let server = find_server_binary();
    let mut cmd = TokioCommand::new(&server);
    cmd.arg("serve")
        .arg("--port")
        .arg(port.to_string())
        .current_dir(cwd)
        .env("PORT", port.to_string())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if !keys.is_empty() {
        cmd.env(
            "SEBENZA_PROJECT_ENV_KEYS",
            keys.into_iter().collect::<Vec<_>>().join(","),
        );
    }

    println!("Starting Sebenza on port {port}...");

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to start {server}: {e}");
            return 1;
        }
    };

    if app {
        let url = format!("http://localhost:{port}");
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            open_app_mode(&url);
        });
    }

    // Forward termination to the child so `kill <sebenza-pid>` doesn't orphan the
    // server (interactive Ctrl-C already reaches both via the process group).
    let terminate = async {
        #[cfg(unix)]
        {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("install SIGTERM handler");
            sigterm.recv().await;
        }
        #[cfg(not(unix))]
        std::future::pending::<()>().await;
    };

    tokio::select! {
        status = child.wait() => status.ok().and_then(|s| s.code()).unwrap_or(0),
        _ = tokio::signal::ctrl_c() => {
            let _ = child.kill().await;
            130
        }
        _ = terminate => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            143
        }
    }
}
