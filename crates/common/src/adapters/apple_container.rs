//! Apple Container **machine** sandbox adapter (`container machine`).
//!
//! Pure builders are unit-tested. Process helpers spawn the `container` CLI and
//! are not invoked from `cargo test`.
//!
//! Machines have no extra VirtioFS/share flags. `--home-mount none` is used only
//! if extra shares become available; until then we fall back to `--home-mount rw`
//! when every sandbox path is under `$HOME`, and refuse otherwise.

use crate::domain::config::MountSpec;
use crate::util::shell::which;
use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

const CONTAINER_OP_TIMEOUT: Duration = Duration::from_secs(120);

/// Options for launching an Apple Container machine.
pub struct LaunchAppleOpts {
    pub branch: String,
    pub wt_dir: String,
    pub main_repo_dir: String,
    pub image: String,
    pub env_passthrough: Vec<String>,
    pub mounts: Vec<MountSpec>,
    pub service_port_envs: Vec<String>,
    pub runtime_env: HashMap<String, String>,
}

/// `--home-mount` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeMount {
    None,
    Ro,
    Rw,
}

impl HomeMount {
    pub fn as_str(self) -> &'static str {
        match self {
            HomeMount::None => "none",
            HomeMount::Ro => "ro",
            HomeMount::Rw => "rw",
        }
    }
}

fn sanitise_branch_for_name(branch: &str) -> String {
    let mut s: String = branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s
        .trim_start_matches(|c: char| !c.is_ascii_alphanumeric())
        .trim_end_matches('-');
    let sliced: String = s.chars().take(46).collect();
    if sliced.is_empty() {
        "x".to_string()
    } else {
        sliced
    }
}

/// `sebenza-<branch>-<millis>` — same scheme as LXC/Docker.
pub fn instance_name(branch: &str, millis: i64) -> String {
    format!("sebenza-{}-{millis}", sanitise_branch_for_name(branch))
}

fn instance_prefix(branch: &str) -> String {
    format!("sebenza-{}-", sanitise_branch_for_name(branch))
}

/// Names from `container machine list -q` that belong to `branch`.
pub fn matching_names(listing: &str, branch: &str) -> Vec<String> {
    let prefix = instance_prefix(branch);
    listing
        .lines()
        .map(str::trim)
        .filter(|n| {
            n.starts_with(&prefix)
                && n.len() > prefix.len()
                && n[prefix.len()..].chars().all(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
        .collect()
}

pub fn missing_container_cli_error() -> String {
    "Apple Container CLI is not installed (container not found). Install it from https://github.com/apple/container/releases and run: container system start".to_string()
}

pub fn system_not_running_error() -> String {
    "Apple Container services are not running. Start them with: container system start".to_string()
}

/// The CLI currently has no extra VirtioFS flags on `container machine create`.
pub fn extra_shares_supported() -> bool {
    false
}

pub fn path_under_home(path: &str, home: &str) -> bool {
    let home = home.trim_end_matches('/');
    path == home || path.starts_with(&format!("{home}/"))
}

/// Decide `--home-mount`. Extra shares are unsupported today, so this is `rw`
/// when every path is under `$HOME`, otherwise an error.
pub fn plan_home_mount(home: &str, paths: &[&str]) -> Result<HomeMount, String> {
    if extra_shares_supported() {
        return Ok(HomeMount::None);
    }
    let outside: Vec<&&str> = paths.iter().filter(|p| !path_under_home(p, home)).collect();
    if outside.is_empty() {
        return Ok(HomeMount::Rw);
    }
    Err(format!(
        "Apple Container machines can only share $HOME ({home}); these paths are outside it: {}. Move the worktree under your home directory, or use runtime: host.",
        outside
            .iter()
            .map(|p| p.as_ref())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// `container machine create` argv (not including the `container` binary).
pub fn machine_create_args(name: &str, image: &str, home_mount: HomeMount) -> Vec<String> {
    vec![
        "machine".into(),
        "create".into(),
        "--name".into(),
        name.into(),
        "--home-mount".into(),
        home_mount.as_str().into(),
        image.into(),
    ]
}

/// `container machine run` argv for a pane/exec.
pub fn machine_run_args(
    name: &str,
    uid: u32,
    gid: u32,
    workdir: &str,
    command: &[String],
) -> Vec<String> {
    let mut args = vec![
        "machine".into(),
        "run".into(),
        "-n".into(),
        name.into(),
        "--uid".into(),
        uid.to_string(),
        "--gid".into(),
        gid.to_string(),
        "--workdir".into(),
        workdir.into(),
        "--".into(),
    ];
    args.extend(command.iter().cloned());
    args
}

pub fn machine_delete_args(name: &str) -> Vec<String> {
    vec!["machine".into(), "delete".into(), name.into()]
}

pub fn machine_list_args() -> Vec<String> {
    vec!["machine".into(), "list".into(), "-q".into()]
}

pub fn machine_stop_args(name: &str) -> Vec<String> {
    vec!["machine".into(), "stop".into(), name.into()]
}

pub fn system_status_args() -> Vec<String> {
    vec!["system".into(), "status".into()]
}

fn run_container(args: &[String], timeout: Duration) -> Result<String, String> {
    let mut child = Command::new("container")
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("container spawn failed: {e}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child
                    .stdout
                    .take()
                    .and_then(|mut s| {
                        use std::io::Read;
                        let mut b = String::new();
                        s.read_to_string(&mut b).ok().map(|_| b)
                    })
                    .unwrap_or_default();
                if status.success() {
                    return Ok(stdout);
                }
                let stderr = child
                    .stderr
                    .take()
                    .and_then(|mut s| {
                        use std::io::Read;
                        let mut b = String::new();
                        s.read_to_string(&mut b).ok().map(|_| b)
                    })
                    .unwrap_or_default();
                return Err(format!(
                    "container {} failed (exit {}): {}",
                    args.first().cloned().unwrap_or_default(),
                    status.code().unwrap_or(-1),
                    stderr.trim()
                ));
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    return Err("container command timed out".to_string());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn require_container_cli() -> Result<(), String> {
    if which("container") {
        Ok(())
    } else {
        Err(missing_container_cli_error())
    }
}

fn require_system_running() -> Result<(), String> {
    match run_container(&system_status_args(), Duration::from_secs(15)) {
        Ok(_) => Ok(()),
        Err(_) => Err(system_not_running_error()),
    }
}

fn list_machine_ids() -> String {
    run_container(&machine_list_args(), Duration::from_secs(15)).unwrap_or_default()
}

/// First existing machine for `branch`.
pub fn find_instance(branch: &str) -> Option<String> {
    matching_names(&list_machine_ids(), branch)
        .into_iter()
        .next()
}

fn remove_by_name(name: &str) {
    let _ = run_container(&machine_stop_args(name), Duration::from_secs(30));
    let _ = run_container(&machine_delete_args(name), Duration::from_secs(30));
}

/// Delete every machine for `branch`.
pub fn remove_instance(branch: &str) {
    for name in matching_names(&list_machine_ids(), branch) {
        remove_by_name(&name);
    }
}

fn sandbox_paths(opts: &LaunchAppleOpts, home: &str) -> Vec<String> {
    let mut paths = vec![opts.wt_dir.clone(), opts.main_repo_dir.clone()];
    for mount in &opts.mounts {
        let host = if let Some(rest) = mount.host_path.strip_prefix('~') {
            format!("{home}{rest}")
        } else {
            mount.host_path.clone()
        };
        if host.starts_with('/') {
            paths.push(host);
        }
    }
    paths
}

/// Launch or reuse an Apple Container machine; returns the instance name.
pub fn launch_instance(opts: &LaunchAppleOpts) -> Result<String, String> {
    require_container_cli()?;
    require_system_running()?;
    if let Some(existing) = find_instance(&opts.branch) {
        return Ok(existing);
    }
    if opts.image.trim().is_empty() {
        return Err("sandbox image is required but was empty".to_string());
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/user".to_string());
    let paths = sandbox_paths(opts, &home);
    let path_refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    let home_mount = plan_home_mount(&home, &path_refs)?;
    let name = instance_name(&opts.branch, chrono::Utc::now().timestamp_millis());
    if let Err(e) = run_container(
        &machine_create_args(&name, &opts.image, home_mount),
        CONTAINER_OP_TIMEOUT,
    ) {
        remove_by_name(&name);
        return Err(e);
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_name_sanitized_like_lxc() {
        assert_eq!(sanitise_branch_for_name("feature/x y"), "feature-x-y");
        assert_eq!(instance_name("feature/x", 1), "sebenza-feature-x-1");
    }

    #[test]
    fn matching_names_keep_only_the_branch_prefix_and_digits() {
        let listing = "\
sebenza-feature-x-1
sebenza-feature-x-99
sebenza-other-2
sebenza-feature-x-nope
";
        assert_eq!(
            matching_names(listing, "feature/x"),
            vec!["sebenza-feature-x-1", "sebenza-feature-x-99"]
        );
    }

    #[test]
    fn create_args_use_home_mount_and_oci_image() {
        let args = machine_create_args("sebenza-feat-1", "ubuntu:24.04", HomeMount::None);
        assert_eq!(
            args,
            vec![
                "machine",
                "create",
                "--name",
                "sebenza-feat-1",
                "--home-mount",
                "none",
                "ubuntu:24.04"
            ]
        );
        let rw = machine_create_args("n", "alpine:latest", HomeMount::Rw);
        assert!(rw.contains(&"--home-mount".into()));
        assert!(rw.contains(&"rw".into()));
    }

    #[test]
    fn extra_shares_are_unsupported_so_home_mount_falls_back_to_rw_under_home() {
        assert!(!extra_shares_supported());
        let home = "/Users/u";
        assert_eq!(
            plan_home_mount(home, &["/Users/u/git/repo", "/Users/u/git/repo/__wt/feat"]).unwrap(),
            HomeMount::Rw
        );
        let err = plan_home_mount(home, &["/tmp/outside", "/Users/u/git/repo"]).unwrap_err();
        assert!(err.contains("/tmp/outside"), "{err}");
        assert!(err.contains("$HOME") || err.contains("/Users/u"), "{err}");
    }

    #[test]
    fn run_args_set_uid_gid_workdir() {
        let args = machine_run_args(
            "sebenza-feat-1",
            501,
            20,
            "/Users/u/git/repo/__wt/feat",
            &["/bin/sh".into(), "-c".into(), "pwd".into()],
        );
        assert_eq!(
            args,
            vec![
                "machine",
                "run",
                "-n",
                "sebenza-feat-1",
                "--uid",
                "501",
                "--gid",
                "20",
                "--workdir",
                "/Users/u/git/repo/__wt/feat",
                "--",
                "/bin/sh",
                "-c",
                "pwd"
            ]
        );
    }

    #[test]
    fn delete_and_status_argv() {
        assert_eq!(
            machine_delete_args("sebenza-feat-1"),
            vec!["machine", "delete", "sebenza-feat-1"]
        );
        assert_eq!(machine_list_args(), vec!["machine", "list", "-q"]);
        assert_eq!(system_status_args(), vec!["system", "status"]);
        let missing = missing_container_cli_error();
        assert!(missing.contains("container system start"), "{missing}");
        let down = system_not_running_error();
        assert!(down.contains("container system start"), "{down}");
    }
}
