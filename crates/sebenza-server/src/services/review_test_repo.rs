//! Test fixture: a real git repository with a `main` branch holding a Sebenza
//! workspace, and a linked worktree on `feat` carrying one track. Track
//! commits carry the `Sebenza-Track` and `Sebenza-Task` trailers, and the
//! track's committed plan gives its one task a boundary covering everything.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const TRACK: &str = "t_20261118";
pub const OTHER_TRACK: &str = "o_20261101";
pub const TASK: &str = "task-1";
/// Where the track's plan.json is committed.
pub const PLAN: &str = ".ai/sebenza/tracks/t_20261118/plan.json";

/// The trailers of a commit on `track` for `task`.
pub fn trailers(track: &str, task: &str) -> String {
    format!("Sebenza-Track: {track}\nSebenza-Task: {task}")
}

/// A plan.json for `TRACK` with one phase of `tasks`, each given as
/// (task id, `files` globs or `None` for no `files` key).
pub fn plan_json(tasks: &[(&str, Option<&[&str]>)]) -> String {
    let tasks: Vec<serde_json::Value> = tasks
        .iter()
        .map(|(id, files)| {
            let mut t = serde_json::json!({"id": id, "name": id, "status": "backlog"});
            if let Some(files) = files {
                t["files"] = serde_json::json!(files);
            }
            t
        })
        .collect();
    let plan = serde_json::json!({
        "$schema": "sebenza-plan-v1",
        "track_id": TRACK,
        "phases": [{"id": "phase-1", "name": "P1", "status": "doing", "tasks": tasks}],
    });
    format!("{}\n", serde_json::to_string_pretty(&plan).unwrap())
}

pub struct TestRepo {
    pub base: PathBuf,
    pub repo: PathBuf,
    pub worktree: PathBuf,
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The `tracks.json` a skill writes: two tracks, in key order and indented
/// like Python's `json.dump(indent=2)`.
pub fn tracks_json(own_status: &str, branch: &str) -> String {
    format!(
        r#"{{
  "$schema": "sebenza-tracks-v1",
  "tracks": [
    {{
      "track_id": "{OTHER_TRACK}",
      "description": "Other",
      "status": "doing",
      "updated_at": "2026-11-01T00:00:00Z"
    }},
    {{
      "track_id": "{TRACK}",
      "description": "Mine",
      "status": "{own_status}",
      "updated_at": "2026-11-18T00:00:00Z",
      "origin": {{
        "branch": "{branch}"
      }}
    }}
  ]
}}
"#
    )
}

/// [`tracks_json`] with no `origin` on the track.
pub fn tracks_json_without_origin(own_status: &str) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&tracks_json(own_status, "")).unwrap();
    for t in v["tracks"].as_array_mut().unwrap() {
        t.as_object_mut().unwrap().remove("origin");
    }
    serde_json::to_string_pretty(&v).unwrap()
}

impl TestRepo {
    pub fn new(name: &str) -> TestRepo {
        let base = std::env::temp_dir().join(format!(
            "sebenza-review-repo-{}-{name}-{}",
            std::process::id(),
            common::util::id::random_ulid()
        ));
        let repo = base.join("repo");
        std::fs::create_dir_all(repo.join(".ai/sebenza/code_styleguides")).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        write(&repo, ".ai/sebenza/index.md", INDEX);
        write(&repo, ".ai/sebenza/workflow.md", "# Workflow\n");
        write(
            &repo,
            ".ai/sebenza/code_styleguides/general.md",
            "# General\n",
        );
        write(&repo, ".ai/sebenza/tracks.json", "{\"tracks\": []}\n");
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        let worktree = base.join("wt-feat");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feat",
                worktree.to_str().unwrap(),
            ],
        );
        let worktree = worktree.canonicalize().unwrap();
        std::fs::create_dir_all(worktree.join(".ai/sebenza/tracks").join(TRACK)).unwrap();
        write(
            &worktree,
            ".ai/sebenza/tracks.json",
            &tracks_json("doing", "feat"),
        );
        write(&worktree, "src.rs", "fn main() {}\n");
        write(&worktree, PLAN, &plan_json(&[(TASK, Some(&["**"]))]));
        commit_all(&worktree, "feat work");
        TestRepo {
            base,
            repo: repo.canonicalize().unwrap(),
            worktree,
        }
    }

    /// A second linked worktree on a new `branch` off `main`, registering
    /// the track (with its directory) in a `tracks.json` of `tracks`.
    pub fn add_worktree(&self, branch: &str, tracks: &str) -> PathBuf {
        let path = self.base.join(format!("wt-{branch}"));
        git(
            &self.repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                branch,
                path.to_str().unwrap(),
                "main",
            ],
        );
        let path = path.canonicalize().unwrap();
        std::fs::create_dir_all(path.join(".ai/sebenza/tracks").join(TRACK)).unwrap();
        write(&path, ".ai/sebenza/tracks.json", tracks);
        path
    }

    pub fn head(&self) -> String {
        git(&self.worktree, &["rev-parse", "HEAD"])
    }

    pub fn main_head(&self) -> String {
        git(&self.repo, &["rev-parse", "main"])
    }

    pub fn tracks_path(&self) -> PathBuf {
        self.worktree.join(".ai/sebenza/tracks.json")
    }

    /// Commit `content` at `rel` on `main`.
    pub fn commit_on_main(&self, rel: &str, content: &str) -> String {
        write(&self.repo, rel, content);
        git(&self.repo, &["add", "-A"]);
        git(&self.repo, &["commit", "-q", "-m", "main change"]);
        self.main_head()
    }

    /// Commit `content` at `rel` on the track branch.
    pub fn commit_on_feat(&self, rel: &str, content: &str) -> String {
        self.commit_bytes_on(&self.worktree, rel, content.as_bytes())
    }

    /// Commit `content` at `rel` on the track branch with `message` as the
    /// whole commit message, trailers included or not.
    pub fn commit_on_feat_with(&self, rel: &str, content: &str, message: &str) -> String {
        write(&self.worktree, rel, content);
        git(&self.worktree, &["add", "-A"]);
        git(&self.worktree, &["commit", "-q", "-m", message]);
        self.head()
    }

    /// Commit raw `bytes` at `rel` on the track branch.
    pub fn commit_bytes_on_feat(&self, rel: &str, bytes: &[u8]) -> String {
        self.commit_bytes_on(&self.worktree, rel, bytes)
    }

    /// Commit raw `bytes` at `rel` on `main`.
    pub fn commit_bytes_on_main(&self, rel: &str, bytes: &[u8]) -> String {
        self.commit_bytes_on(&self.repo, rel, bytes)
    }

    fn commit_bytes_on(&self, dir: &Path, rel: &str, bytes: &[u8]) -> String {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
        commit_all(dir, "change")
    }

    /// Delete `rel` on the track branch and commit.
    pub fn remove_on_feat(&self, rel: &str) -> String {
        git(&self.worktree, &["rm", "-q", rel]);
        commit_all(&self.worktree, "remove")
    }

    /// Commit a symlink at `rel` pointing to `target` on `main`.
    pub fn symlink_on_main(&self, rel: &str, target: &str) -> String {
        let p = self.repo.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(&p);
        std::os::unix::fs::symlink(target, &p).unwrap();
        commit_all(&self.repo, "symlink")
    }

    /// Commit a submodule entry (mode 160000) at `rel` on `main`.
    pub fn gitlink_on_main(&self, rel: &str) -> String {
        let sha = self.main_head();
        let info = format!("160000,{sha},{rel}");
        git(&self.repo, &["update-index", "--add", "--cacheinfo", &info]);
        git(&self.repo, &["commit", "-q", "-m", "gitlink"]);
        self.main_head()
    }

    /// Merge `main` into the track branch with a merge commit.
    pub fn merge_main_into_feat(&self) -> String {
        git(
            &self.worktree,
            &["merge", "-q", "--no-ff", "--no-edit", "main"],
        );
        self.head()
    }

    pub fn track_status(&self, track: &str) -> String {
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(self.tracks_path()).unwrap()).unwrap();
        v["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["track_id"] == track)
            .map(|t| t["status"].as_str().unwrap_or_default().to_string())
            .unwrap_or_default()
    }
}

fn commit_all(dir: &Path, msg: &str) -> String {
    git(dir, &["add", "-A"]);
    let trailers = trailers(TRACK, TASK);
    git(dir, &["commit", "-q", "-m", msg, "-m", &trailers]);
    git(dir, &["rev-parse", "HEAD"])
}

pub fn write(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

const INDEX: &str = "# Project Context\n\n## Workflow\n- [Workflow](./workflow.md)\n- [Code Style Guides](./code_styleguides/)\n\n## Tracks\n- [Tracks Registry](./tracks.json)\n- [Tracks Directory](./tracks/)\n";
