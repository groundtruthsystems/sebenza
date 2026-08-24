//! Classic LXC (`lxc-create` / `lxc-start` / `lxc-attach`) sandbox adapter.
//!
//! Pure builders are unit-tested. Process helpers spawn the `lxc-*` tools and
//! are not invoked from `cargo test`.

use crate::domain::config::MountSpec;
use crate::util::shell::which;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const LXC_OP_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_ID_COUNT: u32 = 65536;
const LXCBR: &str = "lxcbr0";

const RESERVED_ENV: &[&str] = &[
    "HOME",
    "TERM",
    "IS_SANDBOX",
    "SSH_AUTH_SOCK",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_KEY_0",
    "GIT_CONFIG_VALUE_0",
    "GIT_CONFIG_KEY_1",
    "GIT_CONFIG_VALUE_1",
];

/// Options for launching an LXC sandbox.
pub struct LaunchLxcOpts {
    pub branch: String,
    pub wt_dir: String,
    pub main_repo_dir: String,
    pub image: String,
    pub env_passthrough: Vec<String>,
    pub mounts: Vec<MountSpec>,
    pub service_port_envs: Vec<String>,
    pub runtime_env: HashMap<String, String>,
}

/// Host uid/gid plus the user's subid range, used to punch a 1:1 hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LxcIdmap {
    pub host_uid: u32,
    pub host_gid: u32,
    pub subuid_start: u32,
    pub subgid_start: u32,
    pub id_count: u32,
}

/// Parsed `download` template (`lxc-create -t download -- -d -r -a`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTemplate {
    pub distro: String,
    pub release: String,
    pub arch: String,
}

/// Inputs for the pure config builder (no filesystem, no process).
pub struct LxcConfigInput<'a> {
    pub opts: &'a LaunchLxcOpts,
    pub existing_paths: &'a HashSet<String>,
    pub home: &'a str,
    pub passthrough_values: &'a HashMap<String, String>,
    pub ssh_auth_sock: Option<&'a str>,
    pub idmap: LxcIdmap,
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

/// `sebenza-<branch>-<millis>` — same scheme as the Docker adapter.
pub fn instance_name(branch: &str, millis: i64) -> String {
    format!("sebenza-{}-{millis}", sanitise_branch_for_name(branch))
}

fn instance_prefix(branch: &str) -> String {
    format!("sebenza-{}-", sanitise_branch_for_name(branch))
}

/// Names from `lxc-ls -1` that belong to `branch`.
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

/// Error when classic LXC is missing. Mentions LXD if that `lxc` is on PATH.
pub fn missing_classic_lxc_error(lxd_lxc_present: bool) -> String {
    if lxd_lxc_present {
        "found LXD's lxc CLI but not classic LXC (lxc-create). Install classic LXC with: apt install lxc"
            .to_string()
    } else {
        "Classic LXC is not installed (lxc-create not found). Install it with: apt install lxc"
            .to_string()
    }
}

/// Line to append to `/etc/lxc/lxc-usernet`.
pub fn usernet_hint(user: &str) -> String {
    format!("{user} veth {LXCBR} 10")
}

/// Parse `user:start:count` lines (`/etc/subuid` / `/etc/subgid`).
pub fn parse_subid_range(contents: &str, user: &str) -> Option<(u32, u32)> {
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split(':');
        let name = parts.next()?;
        if name != user {
            continue;
        }
        let start = parts.next()?.parse().ok()?;
        let count = parts.next()?.parse().ok()?;
        if count == 0 {
            continue;
        }
        return Some((start, count));
    }
    None
}

/// How many veths `user` may attach to `bridge` (`/etc/lxc/lxc-usernet`).
pub fn parse_usernet_quota(contents: &str, user: &str, bridge: &str) -> Option<u32> {
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let name = parts.next()?;
        let kind = parts.next()?;
        let br = parts.next()?;
        let n = parts.next()?.parse().ok()?;
        if name == user && kind == "veth" && br == bridge && n > 0 {
            return Some(n);
        }
    }
    None
}

fn map_ubuntu_release(token: &str) -> String {
    match token {
        "24.04" => "noble".into(),
        "22.04" => "jammy".into(),
        "20.04" => "focal".into(),
        "24.10" => "oracular".into(),
        "25.04" => "plucky".into(),
        other => other.to_string(),
    }
}

fn map_download_arch(host_arch: &str) -> String {
    match host_arch {
        "x86_64" | "amd64" => "amd64".into(),
        "aarch64" | "arm64" => "arm64".into(),
        other => other.to_string(),
    }
}

/// Parse `ubuntu:24.04` / `ubuntu/noble` into download-template fields.
pub fn parse_download_image(image: &str, host_arch: &str) -> Result<DownloadTemplate, String> {
    let image = image.trim();
    if image.is_empty() {
        return Err("LXC image is empty".to_string());
    }
    let (distro, release_tok) = if let Some((d, r)) = image.split_once(':') {
        (d, r)
    } else if let Some((d, r)) = image.split_once('/') {
        (d, r)
    } else {
        return Err(format!(
            "LXC image {image:?} must look like ubuntu:24.04 or ubuntu/noble"
        ));
    };
    let distro = distro.trim();
    let release_tok = release_tok.trim();
    if distro.is_empty() || release_tok.is_empty() {
        return Err(format!(
            "LXC image {image:?} is missing a distro or release"
        ));
    }
    Ok(DownloadTemplate {
        distro: distro.to_string(),
        release: map_ubuntu_release(release_tok),
        arch: map_download_arch(host_arch),
    })
}

/// `lxc-create` argv (not including the binary).
pub fn lxc_create_args(name: &str, image: &DownloadTemplate) -> Vec<String> {
    vec![
        "-t".into(),
        "download".into(),
        "-n".into(),
        name.into(),
        "--".into(),
        "-d".into(),
        image.distro.clone(),
        "-r".into(),
        image.release.clone(),
        "-a".into(),
        image.arch.clone(),
    ]
}

fn is_valid_env_key(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_valid_port(s: &str) -> bool {
    s.parse::<u32>()
        .map(|n| (1..=65535).contains(&n))
        .unwrap_or(false)
}

fn expand_home(path: &str, home: &str) -> String {
    if let Some(rest) = path.strip_prefix('~') {
        format!("{home}{rest}")
    } else {
        path.to_string()
    }
}

/// fstab dest relative to the container rootfs (`/home/u/wt` → `home/u/wt`).
pub fn rootfs_dest(host_path: &str) -> String {
    host_path.trim_start_matches('/').to_string()
}

fn escape_fstab_field(s: &str) -> String {
    s.replace(' ', r"\040")
}

/// One `lxc.mount.entry` line. Dest is rootfs-relative so the guest path matches the host.
pub fn mount_entry(host_path: &str, writable: bool) -> String {
    let dest = rootfs_dest(host_path);
    let opts = if writable {
        "bind,create=dir"
    } else {
        "bind,ro,create=dir"
    };
    format!(
        "lxc.mount.entry = {} {} none {opts} 0 0",
        escape_fstab_field(host_path),
        escape_fstab_field(&dest)
    )
}

/// Idmap with a 1:1 hole for the host uid and gid. Empty first/last ranges are omitted.
pub fn idmap_lines(map: &LxcIdmap) -> Result<Vec<String>, String> {
    if map.id_count == 0 {
        return Err("subuid/subgid range is empty".to_string());
    }
    if map.host_uid >= map.id_count {
        return Err(format!(
            "host uid {} is outside the subuid range of {}",
            map.host_uid, map.id_count
        ));
    }
    if map.host_gid >= map.id_count {
        return Err(format!(
            "host gid {} is outside the subgid range of {}",
            map.host_gid, map.id_count
        ));
    }
    let mut lines = Vec::new();
    lines.extend(idmap_kind(
        'u',
        map.host_uid,
        map.subuid_start,
        map.id_count,
    ));
    lines.extend(idmap_kind(
        'g',
        map.host_gid,
        map.subgid_start,
        map.id_count,
    ));
    Ok(lines)
}

fn idmap_kind(kind: char, host_id: u32, sub_start: u32, count: u32) -> Vec<String> {
    let mut lines = Vec::new();
    if host_id > 0 {
        lines.push(format!("lxc.idmap = {kind} 0 {sub_start} {host_id}"));
    }
    lines.push(format!("lxc.idmap = {kind} {host_id} {host_id} 1"));
    let after = host_id + 1;
    if after < count {
        let remaining = count - host_id - 1;
        let next_sub = sub_start + host_id;
        lines.push(format!("lxc.idmap = {kind} {after} {next_sub} {remaining}"));
    }
    lines
}

/// Allocated service ports to publish on loopback.
pub fn service_ports(opts: &LaunchLxcOpts) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut ports = Vec::new();
    for port_env in &opts.service_port_envs {
        let Some(port) = opts.runtime_env.get(port_env) else {
            continue;
        };
        if !is_valid_port(port) || !seen.insert(port.clone()) {
            continue;
        }
        ports.push(port.clone());
    }
    ports
}

/// `socat` argv: `127.0.0.1:port` → `container_ip:port`.
pub fn loopback_proxy_args(port: &str, container_ip: &str) -> Vec<String> {
    vec![
        format!("TCP-LISTEN:{port},bind=127.0.0.1,fork,reuseaddr"),
        format!("TCP:{container_ip}:{port}"),
    ]
}

fn environment_lines(input: &LxcConfigInput<'_>) -> Vec<String> {
    let reserved: HashSet<&str> = RESERVED_ENV.iter().copied().collect();
    let wt = &input.opts.wt_dir;
    let repo = &input.opts.main_repo_dir;
    let mut lines = vec![
        format!("lxc.environment = HOME={}", input.home),
        "lxc.environment = TERM=xterm-256color".into(),
        "lxc.environment = IS_SANDBOX=1".into(),
        "lxc.environment = GIT_CONFIG_COUNT=2".into(),
        "lxc.environment = GIT_CONFIG_KEY_0=safe.directory".into(),
        format!("lxc.environment = GIT_CONFIG_VALUE_0={wt}"),
        "lxc.environment = GIT_CONFIG_KEY_1=safe.directory".into(),
        format!("lxc.environment = GIT_CONFIG_VALUE_1={repo}"),
    ];
    let push_kv = |lines: &mut Vec<String>, key: &str, val: &str| {
        if !is_valid_env_key(key) || reserved.contains(key) {
            return;
        }
        lines.push(format!("lxc.environment = {key}={val}"));
    };
    for key in &input.opts.env_passthrough {
        if let Some(val) = input.passthrough_values.get(key) {
            push_kv(&mut lines, key, val);
        }
    }
    for (key, val) in &input.opts.runtime_env {
        push_kv(&mut lines, key, val);
    }
    if let Some(sock) = input.ssh_auth_sock
        && input.existing_paths.contains(sock)
    {
        lines.push(format!("lxc.environment = SSH_AUTH_SOCK={sock}"));
    }
    lines
}

fn mount_lines(input: &LxcConfigInput<'_>) -> Vec<String> {
    let wt = &input.opts.wt_dir;
    let repo = &input.opts.main_repo_dir;
    let home = input.home;
    let mut lines = vec![
        mount_entry(wt, true),
        mount_entry(&format!("{repo}/.git"), true),
        mount_entry(repo, false),
    ];

    let mut extra_guest: HashSet<String> = HashSet::new();
    for mount in &input.opts.mounts {
        let host_path = expand_home(&mount.host_path, home);
        if !host_path.starts_with('/') {
            continue;
        }
        extra_guest.insert(
            mount
                .guest_path
                .clone()
                .unwrap_or_else(|| host_path.clone()),
        );
    }

    let rw_home = [
        format!("{home}/.claude"),
        format!("{home}/.claude.json"),
        format!("{home}/.codex"),
        format!("{home}/.config/opencode"),
        format!("{home}/.local/share/opencode"),
        // One mount covers both grok's credentials (auth.json) and its session
        // transcripts, the same posture as ~/.claude.json and ~/.codex. Splitting it
        // would break either auth or history inside the sandbox.
        format!("{home}/.grok"),
    ];
    for path in rw_home {
        if extra_guest.contains(&path) {
            continue;
        }
        if input.existing_paths.contains(&path) {
            lines.push(mount_entry(&path, true));
        }
    }

    let ro_home = [
        format!("{home}/.gitconfig"),
        format!("{home}/.ssh"),
        format!("{home}/.config/gh"),
    ];
    for path in ro_home {
        if extra_guest.contains(&path) {
            continue;
        }
        if input.existing_paths.contains(&path) {
            lines.push(mount_entry(&path, false));
        }
    }

    if let Some(sock) = input.ssh_auth_sock
        && input.existing_paths.contains(sock)
        && !extra_guest.contains(sock)
    {
        // Sockets cannot use create=dir; still bind at the same path.
        let dest = rootfs_dest(sock);
        lines.push(format!(
            "lxc.mount.entry = {} {} none bind,create=file 0 0",
            escape_fstab_field(sock),
            escape_fstab_field(&dest)
        ));
    }

    for mount in &input.opts.mounts {
        let host_path = expand_home(&mount.host_path, home);
        if !host_path.starts_with('/') {
            continue;
        }
        let guest = mount
            .guest_path
            .clone()
            .unwrap_or_else(|| host_path.clone());
        // Guest override still bind-mounts the host path; dest follows guest.
        let dest = rootfs_dest(&guest);
        let opts = if mount.writable == Some(true) {
            "bind,create=dir"
        } else {
            "bind,ro,create=dir"
        };
        lines.push(format!(
            "lxc.mount.entry = {} {} none {opts} 0 0",
            escape_fstab_field(&host_path),
            escape_fstab_field(&dest)
        ));
    }
    lines
}

/// Extra config appended (after merging) onto a freshly created container.
pub fn build_lxc_extra_config(input: &LxcConfigInput<'_>) -> Result<String, String> {
    let mut lines = idmap_lines(&input.idmap)?;
    lines.push("lxc.net.0.type = veth".into());
    lines.push(format!("lxc.net.0.link = {LXCBR}"));
    lines.push("lxc.net.0.flags = up".into());
    lines.extend(environment_lines(input));
    lines.extend(mount_lines(input));
    lines.push(String::new());
    Ok(lines.join("\n"))
}

/// Drop existing `lxc.idmap` lines (the default 0→subuid map) and append ours.
/// Keep an existing `lxc.net.0` block rather than duplicating it.
pub fn merge_lxc_config(existing: &str, extra: &str) -> String {
    let stripped: Vec<&str> = existing
        .lines()
        .filter(|l| !l.trim_start().starts_with("lxc.idmap"))
        .collect();
    let has_net = stripped
        .iter()
        .any(|l| l.trim_start().starts_with("lxc.net.0.type"));
    let extra_lines: Vec<&str> = extra
        .lines()
        .filter(|l| {
            if l.is_empty() {
                return true;
            }
            if has_net && l.trim_start().starts_with("lxc.net.0.") {
                return false;
            }
            true
        })
        .collect();
    let mut out = stripped.join("\n");
    if !out.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&extra_lines.join("\n"));
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn default_lxc_dir(home: &str) -> PathBuf {
    PathBuf::from(home).join(".local/share/lxc")
}

fn container_config_path(home: &str, name: &str) -> PathBuf {
    default_lxc_dir(home).join(name).join("config")
}

fn require_classic_lxc() -> Result<(), String> {
    if which("lxc-create") {
        return Ok(());
    }
    Err(missing_classic_lxc_error(which("lxc")))
}

fn read_username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".to_string())
}

/// Load the calling user's subid + usernet quota. Errors name the exact fix.
pub fn preflight_unprivileged(
    user: &str,
    subuid: &str,
    subgid: &str,
    usernet: &str,
) -> Result<LxcIdmap, String> {
    let (subuid_start, uid_count) = parse_subid_range(subuid, user).ok_or_else(|| {
        format!("no /etc/subuid range for {user}. Ask an admin to add: {user}:100000:65536")
    })?;
    let (subgid_start, gid_count) = parse_subid_range(subgid, user).ok_or_else(|| {
        format!("no /etc/subgid range for {user}. Ask an admin to add: {user}:100000:65536")
    })?;
    if parse_usernet_quota(usernet, user, LXCBR).is_none() {
        return Err(format!(
            "no lxc-usernet veth quota for {user} on {LXCBR}. Ask an admin to add this line to /etc/lxc/lxc-usernet:\n{}",
            usernet_hint(user)
        ));
    }
    Ok(LxcIdmap {
        host_uid: 0,
        host_gid: 0,
        subuid_start,
        subgid_start,
        id_count: uid_count.min(gid_count),
    })
}

fn run_lxc(bin: &str, args: &[String], timeout: Duration) -> Result<String, String> {
    let mut child = Command::new(bin)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("{bin} spawn failed: {e}"))?;
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
                    "{bin} failed (exit {}): {}",
                    status.code().unwrap_or(-1),
                    stderr.trim()
                ));
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    return Err(format!("{bin} timed out"));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn lxc_ls_output() -> String {
    Command::new("lxc-ls")
        .arg("-1")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
}

fn lxc_info_state(name: &str) -> Option<String> {
    let output = Command::new("lxc-info")
        .args(["-n", name, "-sH"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// First running (else any) instance for `branch`.
pub fn find_instance(branch: &str) -> Option<String> {
    let names = matching_names(&lxc_ls_output(), branch);
    names
        .iter()
        .find(|n| lxc_info_state(n).as_deref() == Some("RUNNING"))
        .cloned()
        .or_else(|| names.into_iter().next())
}

fn remove_by_name(name: &str) {
    let _ = Command::new("lxc-stop").args(["-n", name, "-k"]).output();
    let _ = Command::new("lxc-destroy")
        .args(["-n", name, "-f"])
        .output();
}

/// Stop and destroy every instance for `branch`.
pub fn remove_instance(branch: &str) {
    for name in matching_names(&lxc_ls_output(), branch) {
        remove_by_name(&name);
    }
}

fn wait_until_running(name: &str) -> Result<(), String> {
    let deadline = Instant::now() + LXC_OP_TIMEOUT;
    loop {
        if lxc_info_state(name).as_deref() == Some("RUNNING") {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("lxc-start timed out waiting for {name}"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn host_ids() -> (u32, u32) {
    let uid = Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let gid = Command::new("id")
        .arg("-g")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    (uid, gid)
}

fn read_file_or_empty(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn host_arch() -> String {
    Command::new("uname")
        .arg("-m")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "x86_64".to_string())
}

/// Launch or reuse an LXC sandbox for a worktree; returns the instance name.
pub fn launch_instance(opts: &LaunchLxcOpts) -> Result<String, String> {
    require_classic_lxc()?;
    if let Some(existing) = find_instance(&opts.branch) {
        if lxc_info_state(&existing).as_deref() != Some("RUNNING") {
            run_lxc(
                "lxc-start",
                &["-n".into(), existing.clone(), "-d".into()],
                LXC_OP_TIMEOUT,
            )?;
            wait_until_running(&existing)?;
        }
        return Ok(existing);
    }
    if opts.image.trim().is_empty() {
        return Err("sandbox image is required but was empty".to_string());
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let user = read_username();
    let mut idmap = preflight_unprivileged(
        &user,
        &read_file_or_empty(Path::new("/etc/subuid")),
        &read_file_or_empty(Path::new("/etc/subgid")),
        &read_file_or_empty(Path::new("/etc/lxc/lxc-usernet")),
    )?;
    let (uid, gid) = host_ids();
    idmap.host_uid = uid;
    idmap.host_gid = gid;

    let template = parse_download_image(&opts.image, &host_arch())?;
    let name = instance_name(&opts.branch, chrono::Utc::now().timestamp_millis());
    if let Err(e) = run_lxc(
        "lxc-create",
        &lxc_create_args(&name, &template),
        LXC_OP_TIMEOUT,
    ) {
        remove_by_name(&name);
        return Err(e);
    }

    let mut existing_paths = HashSet::new();
    for p in [
        format!("{home}/.gitconfig"),
        format!("{home}/.ssh"),
        format!("{home}/.config/gh"),
        format!("{home}/.claude"),
        format!("{home}/.claude.json"),
        format!("{home}/.codex"),
        format!("{home}/.config/opencode"),
        format!("{home}/.local/share/opencode"),
        // One mount covers both grok's credentials (auth.json) and its session
        // transcripts, the same posture as ~/.claude.json and ~/.codex. Splitting it
        // would break either auth or history inside the sandbox.
        format!("{home}/.grok"),
    ] {
        if Path::new(&p).exists() {
            existing_paths.insert(p);
        }
    }
    let ssh_auth_sock = std::env::var("SSH_AUTH_SOCK")
        .ok()
        .filter(|s| Path::new(s).exists());
    if let Some(sock) = &ssh_auth_sock {
        existing_paths.insert(sock.clone());
    }

    let mut passthrough_values = HashMap::new();
    for key in &opts.env_passthrough {
        if let Ok(val) = std::env::var(key) {
            passthrough_values.insert(key.clone(), val);
        }
    }

    let extra = match build_lxc_extra_config(&LxcConfigInput {
        opts,
        existing_paths: &existing_paths,
        home: &home,
        passthrough_values: &passthrough_values,
        ssh_auth_sock: ssh_auth_sock.as_deref(),
        idmap,
    }) {
        Ok(c) => c,
        Err(e) => {
            remove_by_name(&name);
            return Err(e);
        }
    };

    let cfg_path = container_config_path(&home, &name);
    let existing_cfg = read_file_or_empty(&cfg_path);
    let merged = merge_lxc_config(&existing_cfg, &extra);
    if let Err(e) = std::fs::write(&cfg_path, merged) {
        remove_by_name(&name);
        return Err(format!("failed to write LXC config: {e}"));
    }

    if let Err(e) = run_lxc(
        "lxc-start",
        &["-n".into(), name.clone(), "-d".into()],
        LXC_OP_TIMEOUT,
    ) {
        remove_by_name(&name);
        return Err(e);
    }
    if let Err(e) = wait_until_running(&name) {
        remove_by_name(&name);
        return Err(e);
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> LaunchLxcOpts {
        LaunchLxcOpts {
            branch: "feature/x".to_string(),
            wt_dir: "/home/u/repo/__wt/feat".to_string(),
            main_repo_dir: "/home/u/repo".to_string(),
            image: "ubuntu:24.04".to_string(),
            env_passthrough: vec!["GITHUB_TOKEN".into(), "HOME".into()],
            mounts: vec![],
            service_port_envs: vec!["PORT".into()],
            runtime_env: HashMap::from([
                ("PORT".to_string(), "5111".to_string()),
                ("FOO".to_string(), "bar".to_string()),
                ("HOME".to_string(), "/should-not-win".to_string()),
            ]),
        }
    }

    fn idmap() -> LxcIdmap {
        LxcIdmap {
            host_uid: 1000,
            host_gid: 1000,
            subuid_start: 100000,
            subgid_start: 100000,
            id_count: 65536,
        }
    }

    fn input<'a>(
        o: &'a LaunchLxcOpts,
        existing: &'a HashSet<String>,
        pass: &'a HashMap<String, String>,
    ) -> LxcConfigInput<'a> {
        LxcConfigInput {
            opts: o,
            existing_paths: existing,
            home: "/home/u",
            passthrough_values: pass,
            ssh_auth_sock: None,
            idmap: idmap(),
        }
    }

    #[test]
    fn branch_name_sanitized_like_docker() {
        assert_eq!(sanitise_branch_for_name("feature/x y"), "feature-x-y");
        assert_eq!(sanitise_branch_for_name("--weird--"), "weird");
        assert_eq!(instance_name("feature/x", 1), "sebenza-feature-x-1");
    }

    #[test]
    fn matching_names_keep_only_the_branch_prefix_and_digits() {
        let listing = "\
sebenza-feature-x-1
sebenza-feature-x-99
sebenza-other-2
sebenza-feature-x-nope
not-ours
";
        assert_eq!(
            matching_names(listing, "feature/x"),
            vec!["sebenza-feature-x-1", "sebenza-feature-x-99"]
        );
    }

    #[test]
    fn ubuntu_image_parses_to_download_template() {
        let t = parse_download_image("ubuntu:24.04", "x86_64").unwrap();
        assert_eq!(
            t,
            DownloadTemplate {
                distro: "ubuntu".into(),
                release: "noble".into(),
                arch: "amd64".into(),
            }
        );
        let t = parse_download_image("ubuntu/noble", "aarch64").unwrap();
        assert_eq!(t.release, "noble");
        assert_eq!(t.arch, "arm64");
        let args = lxc_create_args("sebenza-x-1", &t);
        assert_eq!(
            args,
            vec![
                "-t",
                "download",
                "-n",
                "sebenza-x-1",
                "--",
                "-d",
                "ubuntu",
                "-r",
                "noble",
                "-a",
                "arm64"
            ]
        );
        assert!(parse_download_image("", "x86_64").is_err());
        assert!(parse_download_image("ubuntu", "x86_64").is_err());
    }

    #[test]
    fn idmap_punches_a_host_uid_hole() {
        let lines = idmap_lines(&idmap()).unwrap();
        assert!(lines.contains(&"lxc.idmap = u 0 100000 1000".into()));
        assert!(lines.contains(&"lxc.idmap = u 1000 1000 1".into()));
        assert!(lines.contains(&"lxc.idmap = u 1001 101000 64535".into()));
        assert!(lines.contains(&"lxc.idmap = g 0 100000 1000".into()));
        assert!(lines.contains(&"lxc.idmap = g 1000 1000 1".into()));
        assert!(lines.contains(&"lxc.idmap = g 1001 101000 64535".into()));
    }

    #[test]
    fn mount_entry_is_same_path_and_rootfs_relative() {
        let rw = mount_entry("/home/u/repo/__wt/feat", true);
        assert_eq!(
            rw,
            "lxc.mount.entry = /home/u/repo/__wt/feat home/u/repo/__wt/feat none bind,create=dir 0 0"
        );
        let ro = mount_entry("/home/u/repo", false);
        assert!(ro.contains("bind,ro,create=dir"), "{ro}");
        assert!(ro.contains(" home/u/repo "), "{ro}");
    }

    #[test]
    fn extra_config_has_core_mounts_env_and_skips_reserved_keys() {
        let o = opts();
        let existing = HashSet::from([
            "/home/u/.gitconfig".to_string(),
            "/home/u/.ssh".to_string(),
            "/home/u/.claude".to_string(),
        ]);
        let pass = HashMap::from([("GITHUB_TOKEN".to_string(), "ghs_x".to_string())]);
        let cfg = build_lxc_extra_config(&input(&o, &existing, &pass)).unwrap();

        assert!(cfg.contains(&mount_entry("/home/u/repo/__wt/feat", true)));
        assert!(cfg.contains(&mount_entry("/home/u/repo/.git", true)));
        assert!(cfg.contains(&mount_entry("/home/u/repo", false)));
        assert!(cfg.contains(&mount_entry("/home/u/.gitconfig", false)));
        assert!(cfg.contains(&mount_entry("/home/u/.ssh", false)));
        assert!(cfg.contains(&mount_entry("/home/u/.claude", true)));
        assert!(
            !cfg.contains("/home/u/.codex"),
            "absent credential dirs must not be mounted: {cfg}"
        );

        assert!(cfg.contains("lxc.environment = IS_SANDBOX=1"));
        assert!(cfg.contains("lxc.environment = HOME=/home/u"));
        assert!(cfg.contains("lxc.environment = GIT_CONFIG_VALUE_0=/home/u/repo/__wt/feat"));
        assert!(cfg.contains("lxc.environment = GITHUB_TOKEN=ghs_x"));
        assert!(cfg.contains("lxc.environment = FOO=bar"));
        assert!(
            !cfg.contains("HOME=/should-not-win"),
            "reserved HOME must not be overridden: {cfg}"
        );
        assert!(cfg.contains("lxc.net.0.type = veth"));
        assert!(cfg.contains("lxc.net.0.link = lxcbr0"));
        assert_eq!(service_ports(&o), vec!["5111"]);
    }

    #[test]
    fn configured_mounts_default_readonly_and_can_be_writable() {
        let mut o = opts();
        o.mounts = vec![
            MountSpec {
                host_path: "~/.npm".into(),
                guest_path: None,
                writable: Some(true),
            },
            MountSpec {
                host_path: "/opt/cache".into(),
                guest_path: Some("/opt/cache".into()),
                writable: None,
            },
        ];
        let existing = HashSet::new();
        let pass = HashMap::new();
        let cfg = build_lxc_extra_config(&input(&o, &existing, &pass)).unwrap();
        assert!(cfg.contains(&mount_entry("/home/u/.npm", true)), "{cfg}");
        assert!(cfg.contains(&mount_entry("/opt/cache", false)), "{cfg}");
    }

    #[test]
    fn merge_replaces_default_idmap_and_does_not_duplicate_net() {
        let existing = "\
lxc.include = /usr/share/lxc/config/common.conf
lxc.idmap = u 0 100000 65536
lxc.idmap = g 0 100000 65536
lxc.net.0.type = veth
lxc.net.0.link = lxcbr0
";
        let extra =
            build_lxc_extra_config(&input(&opts(), &HashSet::new(), &HashMap::new())).unwrap();
        let merged = merge_lxc_config(existing, &extra);
        assert_eq!(
            merged
                .lines()
                .filter(|l| l.contains("lxc.idmap = u 0 "))
                .count(),
            1,
            "{merged}"
        );
        assert!(merged.contains("lxc.idmap = u 1000 1000 1"));
        assert!(!merged.contains("lxc.idmap = u 0 100000 65536"));
        assert_eq!(
            merged
                .lines()
                .filter(|l| l.contains("lxc.net.0.type"))
                .count(),
            1,
            "{merged}"
        );
    }

    #[test]
    fn subid_and_usernet_parsers() {
        let subuid = "# comment\nkevin:100000:65536\nother:200000:65536\n";
        assert_eq!(parse_subid_range(subuid, "kevin"), Some((100000, 65536)));
        assert_eq!(parse_subid_range(subuid, "missing"), None);

        let usernet = "kevin veth lxcbr0 10\n";
        assert_eq!(parse_usernet_quota(usernet, "kevin", "lxcbr0"), Some(10));
        let err = preflight_unprivileged("kevin", subuid, subuid, "").unwrap_err();
        assert!(err.contains("/etc/lxc/lxc-usernet"), "{err}");
        assert!(err.contains("kevin veth lxcbr0 10"), "{err}");
    }

    #[test]
    fn missing_lxc_create_mentions_apt_and_lxd() {
        let msg = missing_classic_lxc_error(false);
        assert!(msg.contains("apt install lxc"), "{msg}");
        assert!(msg.contains("lxc-create"), "{msg}");
        let lxd = missing_classic_lxc_error(true);
        assert!(lxd.contains("LXD"), "{lxd}");
        assert!(lxd.contains("apt install lxc"), "{lxd}");
    }

    #[test]
    fn loopback_proxy_binds_localhost() {
        let args = loopback_proxy_args("5111", "10.0.3.12");
        assert_eq!(
            args,
            vec![
                "TCP-LISTEN:5111,bind=127.0.0.1,fork,reuseaddr",
                "TCP:10.0.3.12:5111"
            ]
        );
    }
}
