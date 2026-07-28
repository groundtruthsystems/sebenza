//! `sebenza-cli project migrate` — fold other running Sebenza servers into this
//! one: register their repos here, then retire their service units. Also
//! provides the best-effort "other servers running" nudge shown before any
//! command that reaches a project.

use common::adapters::instance_registry::{self, InstanceEntry};

use crate::http::Http;
use crate::service_units::{
    InstalledService, Platform, list_installed_services, read_port_from_unit,
};

fn other_instances(port: u16) -> Vec<InstanceEntry> {
    instance_registry::list_live()
        .into_iter()
        .filter(|e| e.port != port)
        .collect()
}

/// Best-effort warning printed to stderr when peer servers are running.
pub fn warn_if_other_instances(port: u16) {
    let others = other_instances(port);
    if others.is_empty() {
        return;
    }
    let ports: Vec<String> = others.iter().map(|e| e.port.to_string()).collect();
    eprintln!(
        "Warning: {} other Sebenza server(s) detected on port(s) {}. Run \
         `sebenza-cli project migrate` to consolidate them into this dashboard.",
        others.len(),
        ports.join(", ")
    );
}

fn find_unit_for_port(services: &[InstalledService], port: u16) -> Option<&InstalledService> {
    services.iter().find(|svc| {
        std::fs::read_to_string(&svc.file_path)
            .ok()
            .and_then(|c| read_port_from_unit(&c))
            == Some(port)
    })
}

fn disable_unit_commands(svc: &InstalledService) -> Vec<Vec<String>> {
    match svc.platform {
        Platform::Linux => vec![
            vec![
                "systemctl".into(),
                "--user".into(),
                "stop".into(),
                svc.name.clone(),
            ],
            vec![
                "systemctl".into(),
                "--user".into(),
                "disable".into(),
                svc.name.clone(),
            ],
        ],
        Platform::Macos => vec![vec![
            "launchctl".into(),
            "unload".into(),
            "-w".into(),
            svc.file_path.to_string_lossy().to_string(),
        ]],
    }
}

fn run(cmd: &[String]) -> (bool, String) {
    match std::process::Command::new(&cmd[0]).args(&cmd[1..]).output() {
        Ok(out) => (
            out.status.success(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        ),
        Err(e) => (false, e.to_string()),
    }
}

pub async fn run_migrate(port: u16) -> i32 {
    let others = other_instances(port);
    if others.is_empty() {
        println!("No other Sebenza servers detected — nothing to migrate.");
        return 0;
    }

    let http = Http::new(port);
    let paths: Vec<String> = others.iter().map(|e| e.project_dir.clone()).collect();
    let (migrated, failed) = match http.migrate_projects(paths).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };

    for p in &migrated {
        println!("Now serving {} ({}) — {}", p.name, p.prefix, p.path);
    }
    let failed_paths: std::collections::HashSet<&str> =
        failed.iter().map(|(p, _)| p.as_str()).collect();
    for (path, error) in &failed {
        eprintln!("Warning: could not add {path}: {error}");
    }

    // Retire each other server's unit.
    let services = list_installed_services();
    for instance in &others {
        if failed_paths.contains(instance.project_dir.as_str()) {
            eprintln!(
                "Skipping retirement of the server on port {} ({}) — its repo wasn't migrated. \
                 Resolve the error above, then stop it yourself.",
                instance.port, instance.project_dir
            );
            continue;
        }
        let Some(unit) = find_unit_for_port(&services, instance.port) else {
            eprintln!(
                "Warning: no installed service found for the server on port {} ({}). If it's a \
                 manual `sebenza-cli serve`, stop it yourself.",
                instance.port, instance.project_dir
            );
            continue;
        };
        let mut all_ok = true;
        for cmd in disable_unit_commands(unit) {
            let (ok, stderr) = run(&cmd);
            if !ok {
                all_ok = false;
                eprintln!("Warning: {} failed: {stderr}", cmd.join(" "));
            }
        }
        if let Err(e) = std::fs::remove_file(&unit.file_path) {
            all_ok = false;
            eprintln!(
                "Warning: could not remove {}: {e}",
                unit.file_path.display()
            );
        }
        if all_ok {
            println!("Retired {} (port {}).", unit.name, instance.port);
        }
    }

    if find_unit_for_port(&list_installed_services(), port).is_none() {
        println!(
            "\nThis server isn't installed as a service. Run `sebenza-cli service install` so it starts on boot."
        );
    }
    println!("\nMigration complete.");
    0
}
