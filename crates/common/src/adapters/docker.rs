//! Docker sandbox runtime — port of `backend-legacy/src/adapters/docker.ts`.
//! `build_docker_run_args` is a pure `docker run` argv builder (unit + parity
//! tested); launch/remove shell out to `docker`.

use crate::domain::config::MountSpec;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

const DOCKER_RUN_TIMEOUT: Duration = Duration::from_secs(120);

/// Options for launching a sandbox container.
pub struct LaunchContainerOpts {
    pub branch: String,
    pub wt_dir: String,
    pub main_repo_dir: String,
    pub image: String,
    pub env_passthrough: Vec<String>,
    pub mounts: Vec<MountSpec>,
    /// Service `portEnv` names (their values come from `runtime_env`).
    pub service_port_envs: Vec<String>,
    pub runtime_env: std::collections::HashMap<String, String>,
}

fn sanitise_branch_for_name(branch: &str) -> String {
    let mut s: String = branch
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') { c } else { '-' })
        .collect();
    // Collapse runs of '-'.
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    // Strip leading non-alnum and trailing '-'.
    let s = s.trim_start_matches(|c: char| !c.is_ascii_alphanumeric()).trim_end_matches('-');
    let sliced: String = s.chars().take(46).collect();
    if sliced.is_empty() { "x".to_string() } else { sliced }
}

fn container_name(branch: &str) -> String {
    format!("sebenza-{}-{}", sanitise_branch_for_name(branch), chrono::Utc::now().timestamp_millis())
}

fn is_valid_port(s: &str) -> bool {
    s.parse::<u32>().map(|n| (1..=65535).contains(&n)).unwrap_or(false)
}

fn is_valid_env_key(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Build the `docker run` argv (pure). `existing_paths` are host paths confirmed
/// to exist (for credential mounts); `home` is the resolved HOME.
#[allow(clippy::too_many_arguments)]
pub fn build_docker_run_args(
    opts: &LaunchContainerOpts,
    existing_paths: &HashSet<String>,
    home: &str,
    name: &str,
    ssh_auth_sock: Option<&str>,
    host_uid: u32,
    host_gid: u32,
) -> Vec<String> {
    let wt = &opts.wt_dir;
    let repo = &opts.main_repo_dir;
    let mut args: Vec<String> = vec![
        "run".into(), "-d".into(),
        "--name".into(), name.into(),
        "-w".into(), wt.clone(),
        "--add-host".into(), "host.docker.internal:host-gateway".into(),
        "--user".into(), format!("{host_uid}:{host_gid}"),
    ];

    // Publish service ports on loopback only.
    let mut seen_ports: HashSet<String> = HashSet::new();
    for port_env in &opts.service_port_envs {
        let Some(port) = opts.runtime_env.get(port_env) else {
            continue;
        };
        if !is_valid_port(port) || !seen_ports.insert(port.clone()) {
            continue;
        }
        args.push("-p".into());
        args.push(format!("127.0.0.1:{port}:{port}"));
    }

    let reserved: HashSet<&str> = [
        "HOME", "TERM", "IS_SANDBOX", "SSH_AUTH_SOCK",
        "GIT_CONFIG_COUNT", "GIT_CONFIG_KEY_0", "GIT_CONFIG_VALUE_0",
        "GIT_CONFIG_KEY_1", "GIT_CONFIG_VALUE_1",
    ].into_iter().collect();
    let e = |args: &mut Vec<String>, kv: String| { args.push("-e".into()); args.push(kv); };
    e(&mut args, "HOME=/root".into());
    e(&mut args, "TERM=xterm-256color".into());
    e(&mut args, "IS_SANDBOX=1".into());
    e(&mut args, "GIT_CONFIG_COUNT=2".into());
    e(&mut args, "GIT_CONFIG_KEY_0=safe.directory".into());
    e(&mut args, format!("GIT_CONFIG_VALUE_0={wt}"));
    e(&mut args, "GIT_CONFIG_KEY_1=safe.directory".into());
    e(&mut args, format!("GIT_CONFIG_VALUE_1={repo}"));

    // envPassthrough from host.
    for key in &opts.env_passthrough {
        if !is_valid_env_key(key) || reserved.contains(key.as_str()) {
            continue;
        }
        if let Ok(val) = std::env::var(key) {
            e(&mut args, format!("{key}={val}"));
        }
    }
    // Generated runtime env.
    for (key, val) in &opts.runtime_env {
        if !is_valid_env_key(key) || reserved.contains(key.as_str()) {
            continue;
        }
        e(&mut args, format!("{key}={val}"));
    }

    // Core mounts.
    let v = |args: &mut Vec<String>, m: String| { args.push("-v".into()); args.push(m); };
    v(&mut args, format!("{wt}:{wt}"));
    v(&mut args, format!("{repo}/.git:{repo}/.git"));
    v(&mut args, format!("{repo}:{repo}:ro"));
    v(&mut args, format!("{home}/.claude:/root/.claude"));
    v(&mut args, format!("{home}/.claude.json:/root/.claude.json"));
    v(&mut args, format!("{home}/.codex:/root/.codex"));

    // Guest paths already covered by configured mounts (explicit mounts win).
    let mut extra_guest: HashSet<String> = HashSet::new();
    for mount in &opts.mounts {
        let host_path = expand_home(&mount.host_path, home);
        if !host_path.starts_with('/') {
            continue;
        }
        extra_guest.insert(mount.guest_path.clone().unwrap_or(host_path));
    }

    // Credential mounts (read-only, only if present and not overridden).
    for (host_path, guest_path) in [
        (format!("{home}/.gitconfig"), "/root/.gitconfig"),
        (format!("{home}/.ssh"), "/root/.ssh"),
        (format!("{home}/.config/gh"), "/root/.config/gh"),
    ] {
        if extra_guest.contains(guest_path) {
            continue;
        }
        if existing_paths.contains(&host_path) {
            v(&mut args, format!("{host_path}:{guest_path}:ro"));
        }
    }

    // SSH agent forwarding.
    if let Some(sock) = ssh_auth_sock
        && existing_paths.contains(sock)
    {
        args.push("--mount".into());
        args.push(format!("type=bind,source={sock},target={sock}"));
        e(&mut args, format!("SSH_AUTH_SOCK={sock}"));
    }

    // Configured mounts.
    for mount in &opts.mounts {
        let host_path = expand_home(&mount.host_path, home);
        if !host_path.starts_with('/') {
            continue;
        }
        let guest_path = mount.guest_path.clone().unwrap_or_else(|| host_path.clone());
        let suffix = if mount.writable == Some(true) { "" } else { ":ro" };
        v(&mut args, format!("{host_path}:{guest_path}{suffix}"));
    }

    args.push(opts.image.clone());
    args.push("sleep".into());
    args.push("infinity".into());
    args
}

fn expand_home(path: &str, home: &str) -> String {
    if let Some(rest) = path.strip_prefix('~') {
        format!("{home}{rest}")
    } else {
        path.to_string()
    }
}

fn get_id(flag: &str) -> u32 {
    Command::new("id")
        .arg(flag)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn path_is_world_accessible_socket(path: &str) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};
        if let Ok(meta) = std::fs::metadata(path) {
            return meta.file_type().is_socket() && (meta.permissions().mode() & 0o007) != 0;
        }
    }
    false
}

/// Launch (or reuse) a sandbox container for a worktree; returns its name.
pub fn launch_container(opts: &LaunchContainerOpts) -> Result<String, String> {
    if let Some(existing) = find_container(&opts.branch) {
        return Ok(existing);
    }
    if opts.image.is_empty() {
        return Err("sandboxConfig.image is required but was empty".to_string());
    }
    let name = container_name(&opts.branch);
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());

    let ssh_auth_sock = std::env::var("SSH_AUTH_SOCK")
        .ok()
        .filter(|s| path_is_world_accessible_socket(s));

    let mut existing_paths: HashSet<String> = HashSet::new();
    for p in [
        format!("{home}/.gitconfig"),
        format!("{home}/.ssh"),
        format!("{home}/.config/gh"),
    ] {
        if Path::new(&p).exists() {
            existing_paths.insert(p);
        }
    }
    if let Some(sock) = &ssh_auth_sock {
        existing_paths.insert(sock.clone());
    }

    let args = build_docker_run_args(
        opts,
        &existing_paths,
        &home,
        &name,
        ssh_auth_sock.as_deref(),
        get_id("-u"),
        get_id("-g"),
    );

    let mut child = Command::new("docker")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("docker run spawn failed: {e}"))?;

    let deadline = Instant::now() + DOCKER_RUN_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(name);
                }
                remove_by_name(&name);
                let stderr = child
                    .stderr
                    .take()
                    .and_then(|mut s| {
                        use std::io::Read;
                        let mut b = String::new();
                        s.read_to_string(&mut b).ok().map(|_| b)
                    })
                    .unwrap_or_default();
                return Err(format!("docker run failed (exit {}): {}", status.code().unwrap_or(-1), stderr.trim()));
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    remove_by_name(&name);
                    return Err("docker run timed out".to_string());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn docker_ps_names(branch: &str, all: bool) -> Vec<String> {
    let prefix = format!("sebenza-{}-", sanitise_branch_for_name(branch));
    let filter = format!("name={prefix}");
    let args: Vec<&str> = if all {
        vec!["ps", "-a", "--filter", &filter, "--format", "{{.Names}}"]
    } else {
        vec!["ps", "--filter", &filter, "--format", "{{.Names}}"]
    };
    let Ok(output) = Command::new("docker").args(&args).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|n| {
            n.starts_with(&prefix) && n[prefix.len()..].chars().all(|c| c.is_ascii_digit()) && n.len() > prefix.len()
        })
        .map(str::to_string)
        .collect()
}

pub fn find_container(branch: &str) -> Option<String> {
    docker_ps_names(branch, false).into_iter().next()
}

fn remove_by_name(name: &str) {
    let _ = Command::new("docker").args(["rm", "-f", name]).output();
}

/// Remove all containers (running or stopped) for a branch.
pub fn remove_container(branch: &str) {
    for name in docker_ps_names(branch, true) {
        remove_by_name(&name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn opts() -> LaunchContainerOpts {
        LaunchContainerOpts {
            branch: "feature/x".to_string(),
            wt_dir: "/repo/__wt/feat".to_string(),
            main_repo_dir: "/repo".to_string(),
            image: "my-image".to_string(),
            env_passthrough: vec![],
            mounts: vec![],
            service_port_envs: vec!["PORT".to_string()],
            runtime_env: HashMap::from([("PORT".to_string(), "5111".to_string()), ("FOO".to_string(), "bar".to_string())]),
        }
    }

    #[test]
    fn run_args_have_core_flags_ports_env_mounts_image() {
        let args = build_docker_run_args(&opts(), &HashSet::new(), "/home/u", "sebenza-feat-1", None, 1000, 1000);
        let joined = args.join(" ");
        assert!(joined.starts_with("run -d --name sebenza-feat-1 -w /repo/__wt/feat"));
        assert!(joined.contains("--user 1000:1000"));
        assert!(joined.contains("-p 127.0.0.1:5111:5111"));
        assert!(joined.contains("-e FOO=bar"));
        assert!(joined.contains("-v /repo/__wt/feat:/repo/__wt/feat"));
        assert!(joined.contains("-v /repo:/repo:ro"));
        assert!(joined.ends_with("my-image sleep infinity"));
        // Reserved keys are not overridden by runtime env.
        assert_eq!(args.iter().filter(|a| a.starts_with("HOME=")).count(), 1);
    }

    #[test]
    fn branch_name_sanitized() {
        assert_eq!(sanitise_branch_for_name("feature/x y"), "feature-x-y");
        assert_eq!(sanitise_branch_for_name("--weird--"), "weird");
    }
}
