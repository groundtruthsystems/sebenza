//! `sebenza-cli update` — refresh installed service units so they point at the current
//! `sebenza-server` binary and restart them. The binary-update mechanism itself
//! depends on how `sebenza-cli` was installed (cargo, release archive, package manager),
//! so this command regenerates + restarts services rather than fetching a build.

use crate::service_units::{
    generate_service_file, list_installed_services, read_port_from_unit, resolve_server_path,
    InstalledService, Platform, ServiceConfig,
};

const DEFAULT_PORT: u16 = 5111;

fn run(cmd: &[&str]) -> (bool, String) {
    match std::process::Command::new(cmd[0]).args(&cmd[1..]).output() {
        Ok(out) => (out.status.success(), String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => (false, e.to_string()),
    }
}

fn regenerate(svc: &InstalledService) -> bool {
    let current = std::fs::read_to_string(&svc.file_path).unwrap_or_default();
    let port = read_port_from_unit(&current).unwrap_or(DEFAULT_PORT);
    // Preserve any baked env by re-parsing is out of scope; regenerate core unit.
    let config = ServiceConfig {
        platform: svc.platform,
        server_path: resolve_server_path(),
        port,
        env: Vec::new(),
    };
    let fresh = generate_service_file(&config);
    if fresh == current {
        return false;
    }
    if std::fs::write(&svc.file_path, &fresh).is_err() {
        return false;
    }
    true
}

fn reload_and_restart(svc: &InstalledService, regenerated: bool) -> Result<(), String> {
    match svc.platform {
        Platform::Linux => {
            if regenerated {
                let (ok, e) = run(&["systemctl", "--user", "daemon-reload"]);
                if !ok {
                    return Err(format!("daemon-reload failed: {e}"));
                }
            }
            let (ok, e) = run(&["systemctl", "--user", "restart", &svc.name]);
            if !ok {
                return Err(format!("restart failed: {e}"));
            }
        }
        Platform::Macos => {
            let path = svc.file_path.to_string_lossy().to_string();
            let _ = run(&["launchctl", "unload", &path]);
            let (ok, e) = run(&["launchctl", "load", "-w", &path]);
            if !ok {
                return Err(format!(
                    "load failed: {e} — service is now unloaded, recover with: launchctl load -w \"{path}\""
                ));
            }
        }
    }
    Ok(())
}

pub fn run_update() -> i32 {
    let services = list_installed_services();
    if services.is_empty() {
        println!("No installed Sebenza services found.");
        print_binary_note();
        return 0;
    }
    println!("Refreshing {} installed Sebenza service(s)...", services.len());
    for svc in &services {
        let regenerated = regenerate(svc);
        match reload_and_restart(svc, regenerated) {
            Ok(()) => {
                let what = if regenerated { "regenerated unit, restarted" } else { "restarted" };
                println!("  {}: {what}", svc.name);
            }
            Err(e) => println!("  {}: failed — {e}", svc.name),
        }
    }
    print_binary_note();
    0
}

fn print_binary_note() {
    println!(
        "\nTo update the `sebenza-cli`/`sebenza-server` binaries themselves, re-run your install method \
         (e.g. `cargo install --path crates/sebenza-cli` from a checkout, or fetch the latest release)."
    );
}
