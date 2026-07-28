#![allow(dead_code)]

mod adapters;
mod server;
mod services;

// Reusable modules live in the `common` crate; alias them at the crate root so
// existing `crate::config` / `crate::domain` / `crate::util` paths keep working.
pub use common::{config, domain, util};

use adapters::projects_registry::ProjectsRegistry;
use adapters::terminal::TerminalManager;
use clap::{Parser, Subcommand};
use server::AppState;
use services::project_manager::ProjectManager;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "sebenza-server", about = "Sebenza backend daemon (Rust)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP/WebSocket server.
    Serve {
        /// Port to bind (defaults to $PORT, then 5111).
        #[arg(long)]
        port: Option<u16>,
        /// Host to bind (defaults to $SEBENZA_HOST, then 127.0.0.1).
        ///
        /// Loopback is the default deliberately: most routes are unauthenticated,
        /// so binding all interfaces exposes worktree creation, the terminal PTY,
        /// and agent control to anything that can reach the port. Pass
        /// `--host 0.0.0.0` to opt in explicitly.
        #[arg(long)]
        host: Option<String>,
    },
}

/// Default bind host. Loopback, not `0.0.0.0` — see `Command::Serve::host`.
const DEFAULT_BIND_HOST: &str = "127.0.0.1";

/// Resolve the bind address from the flag, then the environment, then the loopback
/// default. An unparseable host falls back to the default rather than binding
/// something unintended.
fn resolve_bind_addr(host_flag: Option<&str>, host_env: Option<&str>, port: u16) -> SocketAddr {
    let raw = host_flag
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| host_env.map(str::trim).filter(|s| !s.is_empty()))
        .unwrap_or(DEFAULT_BIND_HOST);

    match raw.parse::<std::net::IpAddr>() {
        Ok(ip) => SocketAddr::new(ip, port),
        Err(_) => {
            tracing::warn!("invalid bind host {raw:?}; falling back to {DEFAULT_BIND_HOST}");
            SocketAddr::new(DEFAULT_BIND_HOST.parse().expect("valid default host"), port)
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Serve { port, host } => serve(port, host).await,
    }
}

async fn serve(port_opt: Option<u16>, host_opt: Option<String>) -> anyhow::Result<()> {
    let port = port_opt
        .or_else(|| std::env::var("PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(5111);
    let host_env = std::env::var("SEBENZA_HOST").ok();
    let addr = resolve_bind_addr(host_opt.as_deref(), host_env.as_deref(), port);

    let cwd = std::env::current_dir()?;
    let project_dir = config::project_root(&cwd.to_string_lossy());

    let control_base_url = format!("http://127.0.0.1:{port}");
    let registry = ProjectsRegistry::new();
    let manager = Arc::new(ProjectManager::new(registry, control_base_url));
    // Load persisted projects, then serve the launch cwd for this session only.
    manager.load_persisted();
    let launch = manager.add_ephemeral(&project_dir);

    let terminal = Arc::new(TerminalManager::new(port));
    // Reap orphaned grouped sessions from previous runs before serving.
    terminal.cleanup_stale_sessions();
    let frontend_dist = resolve_frontend_dist(&project_dir);

    let state = AppState {
        manager,
        terminal,
        agent_stream: Arc::new(services::agent_stream::AgentStreamManager::new()),
        project_inits: Arc::new(services::project_init_service::ProjectInitTracker::new()),
        frontend_dist,
    };

    server::spawn_background_loops(state.clone());
    let app = server::build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(
        "Sebenza serve: http://localhost:{port} (project: {project_dir}, prefix: /{})",
        launch.prefix
    );
    if !addr.ip().is_loopback() {
        tracing::warn!(
            "bound {addr} — NOT loopback. Most routes are unauthenticated, so the dashboard, \
             terminal PTY and agent control are reachable by anything that can reach this port."
        );
    }

    // Self-register as a migration sensor so peer servers (and `sebenza-cli project
    // migrate`) can discover this instance; deregister on graceful shutdown.
    let pid = std::process::id();
    let self_entry = adapters::instance_registry::InstanceEntry {
        port,
        project_dir: project_dir.clone(),
        pid,
    };
    adapters::instance_registry::register(&self_entry);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    adapters::instance_registry::deregister(port, Some(pid));
    Ok(())
}

/// Resolve when the process receives SIGINT or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

/// Optional on-disk SPA override. The frontend is embedded in the binary by
/// default (see `server::FrontendAssets`); setting `$SEBENZA_FRONTEND_DIST` to a
/// directory serves from disk instead (handy for iterating on a fresh build
/// without recompiling). `None` → serve the embedded bundle.
fn resolve_frontend_dist(_project_dir: &str) -> Option<PathBuf> {
    let dir = std::env::var("SEBENZA_FRONTEND_DIST").ok()?;
    let path = PathBuf::from(dir);
    path.is_dir().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_loopback_when_nothing_is_set() {
        let addr = resolve_bind_addr(None, None, 5111);
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert_eq!(addr.port(), 5111);
        assert!(addr.ip().is_loopback(), "default bind must not be reachable off-host");
    }

    #[test]
    fn all_interfaces_require_an_explicit_opt_in() {
        let addr = resolve_bind_addr(Some("0.0.0.0"), None, 5111);
        assert_eq!(addr.ip().to_string(), "0.0.0.0");
        assert!(!addr.ip().is_loopback());
    }

    #[test]
    fn env_is_honoured_and_the_flag_wins_over_it() {
        assert_eq!(resolve_bind_addr(None, Some("0.0.0.0"), 80).ip().to_string(), "0.0.0.0");
        assert_eq!(
            resolve_bind_addr(Some("127.0.0.1"), Some("0.0.0.0"), 80).ip().to_string(),
            "127.0.0.1",
            "an explicit --host must override the environment"
        );
    }

    #[test]
    fn blank_and_invalid_hosts_fall_back_to_loopback() {
        for bad in ["", "   ", "not-an-ip", "example.com"] {
            let addr = resolve_bind_addr(Some(bad), None, 5111);
            assert!(
                addr.ip().is_loopback(),
                "host {bad:?} must fall back to loopback, got {}",
                addr.ip()
            );
        }
    }

    #[test]
    fn ipv6_loopback_is_accepted() {
        let addr = resolve_bind_addr(Some("::1"), None, 5111);
        assert!(addr.ip().is_loopback());
        assert!(addr.is_ipv6());
    }
}
