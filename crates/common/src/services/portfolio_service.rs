//! Cross-project portfolio: the `sebenza` plugin's daemon registry
//! (`~/.ai/sebenza/registry.json`) resolved into each project's tracks.
//!
//! Per the plugin's daemon spec a project whose path or `tracks.json` has gone
//! missing is **reported with a warning, never fatal** — one bad entry must not
//! sink the whole view, so every registry entry comes back with a `status` and
//! the reader decides how to render it.

use crate::adapters::fs::{TrackFileError, read_track_file_in};
use crate::adapters::sebenza_registry::{RegistryRead, SebenzaRegistry};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    /// Path and tracks.json both resolve and parse.
    Ok,
    /// The registered project directory is gone.
    MissingPath,
    /// The project exists but has no readable tracks.json (orphaned workspace).
    MissingTracks,
    /// tracks.json is present but not valid JSON.
    InvalidTracks,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortfolioProject {
    pub name: String,
    pub path: String,
    pub tracks_file: String,
    pub registered_at: Option<String>,
    pub last_synced: Option<String>,
    pub status: ProjectStatus,
    /// The parsed `tracks.json`, untyped for the same reason as
    /// [`crate::adapters::fs::read_tracks`]: the schema lives in the frontend.
    pub tracks: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Portfolio {
    pub registry_path: String,
    /// False when the plugin has never registered anything on this machine.
    pub exists: bool,
    /// Set when the registry file itself is unreadable (corrupt JSON).
    pub error: Option<String>,
    pub projects: Vec<PortfolioProject>,
}

/// The schema says `tracks_file` is absolute; tolerate a relative one by
/// resolving it against the project root rather than the server's cwd.
fn resolve_tracks_file(project_path: &str, tracks_file: &str) -> PathBuf {
    let p = Path::new(tracks_file);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        Path::new(project_path).join(p)
    }
}

/// The Sebenza workspace dir for a registered project — the parent of its
/// `tracks.json`, which is what track `*_path` values are relative to.
fn workspace_dir(project: &PortfolioProject) -> PathBuf {
    resolve_tracks_file(&project.path, &project.tracks_file)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| Path::new(&project.path).join(".ai").join("sebenza"))
}

pub fn load_portfolio() -> Portfolio {
    load_from(&SebenzaRegistry::new())
}

fn load_from(registry: &SebenzaRegistry) -> Portfolio {
    let registry_path = registry.path();
    match registry.read() {
        RegistryRead::Absent => Portfolio {
            registry_path,
            exists: false,
            error: None,
            projects: Vec::new(),
        },
        RegistryRead::Corrupt(error) => Portfolio {
            registry_path,
            exists: true,
            error: Some(error),
            projects: Vec::new(),
        },
        RegistryRead::Ok(file) => Portfolio {
            registry_path,
            exists: true,
            error: None,
            projects: file.projects.into_iter().map(resolve_project).collect(),
        },
    }
}

fn resolve_project(entry: crate::adapters::sebenza_registry::RegistryProject) -> PortfolioProject {
    let tracks_path = resolve_tracks_file(&entry.path, &entry.tracks_file);
    let (status, tracks, error) = if !Path::new(&entry.path).is_dir() {
        (
            ProjectStatus::MissingPath,
            None,
            Some(format!("Project directory not found: {}", entry.path)),
        )
    } else {
        match std::fs::read_to_string(&tracks_path) {
            Err(e) => (ProjectStatus::MissingTracks, None, Some(e.to_string())),
            Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(value) => (ProjectStatus::Ok, Some(value), None),
                Err(e) => (ProjectStatus::InvalidTracks, None, Some(e.to_string())),
            },
        }
    };

    PortfolioProject {
        name: entry.name,
        path: entry.path,
        tracks_file: entry.tracks_file,
        registered_at: entry.registered_at,
        last_synced: entry.last_synced,
        status,
        tracks,
        error,
    }
}

/// Read a track artifact belonging to a *registered* project, addressed by its
/// exact registry `path`. Scoping to a registry entry first, then reusing the
/// workspace traversal guard, means this can only ever read under a project the
/// plugin registered — it is not a general filesystem endpoint.
pub fn read_registry_track_file(project_path: &str, rel: &str) -> Result<String, TrackFileError> {
    let portfolio = load_portfolio();
    let project = portfolio
        .projects
        .iter()
        .find(|p| p.path == project_path)
        .ok_or(TrackFileError::NotFound)?;
    read_track_file_in(&workspace_dir(project), rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sebenza-portfolio-{tag}-{}",
            crate::util::id::random_hex(8)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A healthy project and an orphaned one both survive the load, with only
    /// the broken one flagged — the "skip with a warning, never fatal" rule.
    #[test]
    fn reports_healthy_and_broken_projects_side_by_side() {
        let root = temp_dir("root");

        let good = root.join("good");
        let good_ws = good.join(".ai").join("sebenza");
        fs::create_dir_all(&good_ws).unwrap();
        fs::write(
            good_ws.join("tracks.json"),
            r#"{"tracks":[{"track_id":"a_20260727"}]}"#,
        )
        .unwrap();

        // Exists on disk, but its workspace was never created.
        let orphan = root.join("orphan");
        fs::create_dir_all(&orphan).unwrap();

        let registry_file = root.join("registry.json");
        fs::write(
            &registry_file,
            format!(
                r#"{{"version":"1.0","projects":[
                    {{"name":"good","path":"{good}","tracks_file":"{good_tracks}"}},
                    {{"name":"orphan","path":"{orphan}","tracks_file":"{orphan_tracks}"}},
                    {{"name":"gone","path":"{root}/nope","tracks_file":"{root}/nope/.ai/sebenza/tracks.json"}}
                ]}}"#,
                good = good.display(),
                good_tracks = good_ws.join("tracks.json").display(),
                orphan = orphan.display(),
                orphan_tracks = orphan.join(".ai/sebenza/tracks.json").display(),
                root = root.display(),
            ),
        )
        .unwrap();

        let portfolio = load_from(&SebenzaRegistry::with_file(registry_file));
        assert!(portfolio.exists);
        assert!(portfolio.error.is_none());
        assert_eq!(portfolio.projects.len(), 3, "no project may be dropped");

        assert_eq!(portfolio.projects[0].status, ProjectStatus::Ok);
        assert_eq!(
            portfolio.projects[0].tracks.as_ref().unwrap()["tracks"][0]["track_id"],
            "a_20260727"
        );

        assert_eq!(portfolio.projects[1].status, ProjectStatus::MissingTracks);
        assert!(portfolio.projects[1].error.is_some());

        assert_eq!(portfolio.projects[2].status, ProjectStatus::MissingPath);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn invalid_tracks_json_is_flagged_not_dropped() {
        let root = temp_dir("invalid");
        let ws = root.join("p").join(".ai").join("sebenza");
        fs::create_dir_all(&ws).unwrap();
        fs::write(ws.join("tracks.json"), "{ oops").unwrap();

        let registry_file = root.join("registry.json");
        fs::write(
            &registry_file,
            format!(
                r#"{{"version":"1.0","projects":[{{"name":"p","path":"{p}","tracks_file":"{t}"}}]}}"#,
                p = root.join("p").display(),
                t = ws.join("tracks.json").display(),
            ),
        )
        .unwrap();

        let portfolio = load_from(&SebenzaRegistry::with_file(registry_file));
        assert_eq!(portfolio.projects.len(), 1);
        assert_eq!(portfolio.projects[0].status, ProjectStatus::InvalidTracks);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn absent_registry_yields_an_empty_portfolio() {
        let root = temp_dir("absent");
        let portfolio = load_from(&SebenzaRegistry::with_file(root.join("registry.json")));
        assert!(!portfolio.exists);
        assert!(portfolio.projects.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    /// `tracks_file` sits inside the workspace, so the workspace dir is its
    /// parent — that is the root track `*_path` values resolve against.
    #[test]
    fn workspace_dir_is_the_tracks_file_parent() {
        let project = PortfolioProject {
            name: "p".into(),
            path: "/srv/p".into(),
            tracks_file: "/srv/p/.ai/sebenza/tracks.json".into(),
            registered_at: None,
            last_synced: None,
            status: ProjectStatus::Ok,
            tracks: None,
            error: None,
        };
        assert_eq!(workspace_dir(&project), PathBuf::from("/srv/p/.ai/sebenza"));
    }
}
