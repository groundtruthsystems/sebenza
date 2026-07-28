use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// A project Sebenza knows about across restarts. `path` is the resolved git root.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    pub path: String,
    pub name: String,
    pub added_at: u64,
}

pub struct ProjectsRegistry {
    file: PathBuf,
}

fn default_registry_file() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home)
        .join(".ai")
        .join("sebenza")
        .join("projects.json")
}

impl ProjectsRegistry {
    pub fn new() -> Self {
        ProjectsRegistry {
            file: default_registry_file(),
        }
    }

    #[cfg(test)]
    pub fn with_file(file: PathBuf) -> Self {
        ProjectsRegistry { file }
    }

    pub fn list(&self) -> Vec<ProjectEntry> {
        let Ok(raw) = fs::read_to_string(&self.file) else {
            return Vec::new();
        };
        serde_json::from_str::<Vec<ProjectEntry>>(&raw).unwrap_or_default()
    }

    fn write(&self, entries: &[ProjectEntry]) {
        if let Some(parent) = self.file.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(entries) {
            let _ = fs::write(&self.file, format!("{json}\n"));
        }
    }

    /// Upsert by `path` (an existing entry with the same path is replaced).
    pub fn add(&self, entry: ProjectEntry) {
        let mut entries: Vec<ProjectEntry> = self
            .list()
            .into_iter()
            .filter(|e| e.path != entry.path)
            .collect();
        entries.push(entry);
        self.write(&entries);
    }

    pub fn remove(&self, path: &str) {
        let entries = self.list();
        let next: Vec<ProjectEntry> = entries.iter().filter(|e| e.path != path).cloned().collect();
        if next.len() != entries.len() {
            self.write(&next);
        }
    }
}

impl Default for ProjectsRegistry {
    fn default() -> Self {
        Self::new()
    }
}
