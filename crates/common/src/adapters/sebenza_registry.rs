//! Read access to the `sebenza` plugin's user-scoped daemon registry at
//! `~/.ai/sebenza/registry.json` (`$schema: sebenza-registry-v1`) — the index of
//! every Sebenza-enabled project on this machine.
//!
//! Deliberately **read-only**: the plugin owns this file (register, deregister,
//! `last_synced` refresh, corrupt-file recovery). We only observe it.
//!
//! Distinct from [`crate::adapters::projects_registry`], which is *this app's*
//! `~/.ai/sebenza/projects.json` — same directory, different file, different owner.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// One project entry as written by `sebenza-setup`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryProject {
    pub name: String,
    pub path: String,
    /// Absolute path to the project's `.ai/sebenza/tracks.json`, per the schema.
    pub tracks_file: String,
    #[serde(default)]
    pub registered_at: Option<String>,
    #[serde(default)]
    pub last_synced: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryFile {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub projects: Vec<RegistryProject>,
}

/// Outcome of reading the registry. Absence is normal (the plugin has never run
/// here) and is not an error; a corrupt file is reported rather than swallowed,
/// so the UI can say why the portfolio is empty.
#[derive(Debug)]
pub enum RegistryRead {
    Absent,
    Corrupt(String),
    Ok(RegistryFile),
}

pub struct SebenzaRegistry {
    file: PathBuf,
}

fn default_registry_file() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".ai").join("sebenza").join("registry.json")
}

impl SebenzaRegistry {
    pub fn new() -> Self {
        SebenzaRegistry { file: default_registry_file() }
    }

    #[cfg(test)]
    pub fn with_file(file: PathBuf) -> Self {
        SebenzaRegistry { file }
    }

    pub fn path(&self) -> String {
        self.file.to_string_lossy().to_string()
    }

    pub fn read(&self) -> RegistryRead {
        let Ok(raw) = fs::read_to_string(&self.file) else {
            return RegistryRead::Absent;
        };
        match serde_json::from_str::<RegistryFile>(&raw) {
            Ok(parsed) => RegistryRead::Ok(parsed),
            Err(e) => RegistryRead::Corrupt(e.to_string()),
        }
    }
}

impl Default for SebenzaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("sebenza-registry-test-{}", crate::util::id::random_hex(8)));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn absent_registry_is_not_an_error() {
        let reg = SebenzaRegistry::with_file(temp_file("registry.json"));
        assert!(matches!(reg.read(), RegistryRead::Absent));
    }

    #[test]
    fn parses_a_v1_registry() {
        let file = temp_file("registry.json");
        fs::write(
            &file,
            r#"{
              "$schema": "sebenza-registry-v1",
              "version": "1.0",
              "projects": [{
                "name": "demo",
                "path": "/tmp/demo",
                "tracks_file": "/tmp/demo/.ai/sebenza/tracks.json",
                "registered_at": "2026-07-27T12:14:04Z",
                "last_synced": "2026-07-27T12:14:04Z"
              }]
            }"#,
        )
        .unwrap();

        let RegistryRead::Ok(parsed) = SebenzaRegistry::with_file(file.clone()).read() else {
            panic!("expected a parsed registry");
        };
        assert_eq!(parsed.version, "1.0");
        assert_eq!(parsed.projects.len(), 1);
        assert_eq!(parsed.projects[0].name, "demo");
        assert_eq!(parsed.projects[0].tracks_file, "/tmp/demo/.ai/sebenza/tracks.json");
        assert_eq!(parsed.projects[0].last_synced.as_deref(), Some("2026-07-27T12:14:04Z"));

        fs::remove_dir_all(file.parent().unwrap()).ok();
    }

    #[test]
    fn corrupt_registry_reports_why() {
        let file = temp_file("registry.json");
        fs::write(&file, "{ not json").unwrap();
        assert!(matches!(
            SebenzaRegistry::with_file(file.clone()).read(),
            RegistryRead::Corrupt(_)
        ));
        fs::remove_dir_all(file.parent().unwrap()).ok();
    }
}
