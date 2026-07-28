use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceEntry {
    pub port: u16,
    #[serde(rename = "projectDir")]
    pub project_dir: String,
    pub pid: u32,
}

fn registry_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home)
        .join(".ai")
        .join("sebenza")
        .join("instances")
}

fn entry_path(port: u16) -> PathBuf {
    registry_dir().join(format!("{port}.json"))
}

/// A live PID is one whose `/proc/<pid>` exists (Linux): a process we can't
/// signal (EPERM) still counts as alive — its `/proc` entry exists regardless
/// of ownership.
fn is_alive(pid: u32) -> bool {
    PathBuf::from(format!("/proc/{pid}")).exists()
}

fn read_entry(path: &std::path::Path) -> Option<InstanceEntry> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<InstanceEntry>(&raw).ok()
}

/// Register this server. Written to a temp file then renamed for atomicity.
pub fn register(entry: &InstanceEntry) {
    let dir = registry_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let final_path = entry_path(entry.port);
    let tmp_path = dir.join(format!("{}.{}.tmp", entry.port, std::process::id()));
    let Ok(text) = serde_json::to_string_pretty(entry) else {
        return;
    };
    if fs::write(&tmp_path, format!("{text}\n")).is_ok() {
        let _ = fs::rename(&tmp_path, &final_path);
    }
}

/// Delete the entry at `port`. When `expected_pid` is provided, only delete if
/// the entry's pid matches — guards against clobbering a successor that reused
/// the port.
pub fn deregister(port: u16, expected_pid: Option<u32>) {
    if let Some(expected) = expected_pid
        && let Some(entry) = read_entry(&entry_path(port))
        && entry.pid != expected
    {
        return;
    }
    let _ = fs::remove_file(entry_path(port));
}

/// Live instances, pruning entries whose process is gone (best effort).
pub fn list_live() -> Vec<InstanceEntry> {
    let dir = registry_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut live = Vec::new();
    for dirent in entries.flatten() {
        let path = dirent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(entry) = read_entry(&path) else {
            continue;
        };
        if is_alive(entry.pid) {
            live.push(entry);
        } else {
            let _ = fs::remove_file(&path);
        }
    }
    live
}
