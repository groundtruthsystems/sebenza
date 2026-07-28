//! HTTP client for `sebenza-server`. Every `sebenza-cli` command talks to a running server
//! over these endpoints (see `crates/backend/src/server.rs` for the routes).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};

// ── Wire types (camelCase per the server's serde config) ────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectSummary {
    pub prefix: String,
    pub name: String,
    pub path: String,
    pub active: bool,
}

#[derive(Debug, Deserialize)]
struct ProjectsResponse {
    projects: Vec<ProjectSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddProjectResponse {
    pub initializing: bool,
    pub path: String,
    pub project: Option<ProjectSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInit {
    pub path: String,
    pub phase: String,
    pub prefix: Option<String>,
    pub name: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectInitsResponse {
    inits: Vec<ProjectInit>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeTab {
    pub tab_id: String,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrEntry {
    pub number: i64,
    pub url: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSnapshot {
    pub branch: String,
    /// `"main"` for the repository's own checkout, `"linked"` for a worktree.
    /// Defaults for older servers that don't send it.
    #[serde(default = "default_worktree_kind")]
    pub kind: String,
    pub label: Option<String>,
    #[serde(default)]
    pub base_branch: Option<String>,
    pub archived: bool,
    pub profile: Option<String>,
    pub agent_name: Option<String>,
    /// `mux` is the open flag: the tmux session exists.
    pub mux: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub prs: Vec<PrEntry>,
    #[serde(default)]
    pub oneshot: Option<serde_json::Value>,
    #[serde(default)]
    pub tabs: Vec<WorktreeTab>,
    pub active_tab_id: Option<String>,
}

fn default_worktree_kind() -> String {
    "linked".to_string()
}

#[derive(Debug, Deserialize)]
struct WorktreeListResponse {
    worktrees: Vec<WorktreeSnapshot>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsUiMessage {
    pub id: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i64>,
}

fn default_kind() -> String {
    "text".to_string()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationState {
    #[serde(default)]
    messages: Vec<AgentsUiMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationHistoryResponse {
    conversation: ConversationState,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub main_branch: String,
}

#[derive(Debug, Deserialize)]
pub struct ProjectSnapshot {
    pub project: ProjectInfo,
    pub worktrees: Vec<WorktreeSnapshot>,
}

#[derive(Debug, Deserialize)]
struct CreatedTab {
    tab: WorktreeTab,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateWorktreeResponse {
    primary_branch: String,
    branches: Vec<String>,
}

// ── Path helpers (canonical git-root resolution) ────────────────────────────

/// Resolve a directory to its canonical project (git) root — the shared root
/// even from a linked worktree. Returns None when the dir isn't a git work tree.
pub fn resolve_project_root(cwd: &str) -> Option<String> {
    let common = Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if common.status.success() {
        let common_dir = String::from_utf8_lossy(&common.stdout).trim().to_string();
        if !common_dir.is_empty() {
            let joined = Path::new(cwd).join(&common_dir);
            if let Some(parent) = joined.parent() {
                return Some(parent.to_string_lossy().to_string());
            }
        }
    }
    let top = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if top.status.success() {
        let root = String::from_utf8_lossy(&top.stdout).trim().to_string();
        if !root.is_empty() {
            return Some(root);
        }
    }
    None
}

/// Canonicalize a path for equality comparison (collapse symlinks / `.` / `..`),
/// falling back to a best-effort absolute path when the target is gone.
fn canonicalize_path(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| {
            let p = PathBuf::from(path);
            if p.is_absolute() {
                p.to_string_lossy().to_string()
            } else {
                std::env::current_dir()
                    .map(|c| c.join(&p).to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string())
            }
        })
}

fn friendly_connect_error(err: &reqwest::Error, port: u16) -> anyhow::Error {
    if err.is_connect() || err.is_timeout() {
        anyhow!("Could not connect to Sebenza server on port {port}. Is it running?")
    } else {
        anyhow!("{err}")
    }
}

// ── Client ──────────────────────────────────────────────────────────────────

pub struct Http {
    http: reqwest::Client,
    port: u16,
    hub: String,
}

impl Http {
    pub fn new(port: u16) -> Self {
        Http {
            http: reqwest::Client::new(),
            port,
            hub: format!("http://localhost:{port}"),
        }
    }

    /// Turn a response into a JSON value, mapping non-2xx into the server's
    /// `{ "error": ... }` message when present.
    async fn read_json(&self, resp: reqwest::Response) -> Result<Value> {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status.is_success() {
            if body.is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_str(&body).context("invalid JSON from server");
        }
        let msg = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
            .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
        Err(anyhow!(msg))
    }

    async fn get(&self, url: &str) -> Result<Value> {
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| friendly_connect_error(&e, self.port))?;
        self.read_json(resp).await
    }

    async fn post(&self, url: &str, body: Value) -> Result<Value> {
        let resp = self
            .http
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|e| friendly_connect_error(&e, self.port))?;
        self.read_json(resp).await
    }

    async fn put(&self, url: &str, body: Value) -> Result<Value> {
        let resp = self
            .http
            .put(url)
            .json(&body)
            .send()
            .await
            .map_err(|e| friendly_connect_error(&e, self.port))?;
        self.read_json(resp).await
    }

    async fn delete(&self, url: &str) -> Result<Value> {
        let resp = self
            .http
            .delete(url)
            .send()
            .await
            .map_err(|e| friendly_connect_error(&e, self.port))?;
        self.read_json(resp).await
    }

    // ── Hub-level (project registry) ─────────────────────────────────────────

    pub async fn fetch_projects(&self) -> Result<Vec<ProjectSummary>> {
        let v = self.get(&format!("{}/api/projects", self.hub)).await?;
        Ok(serde_json::from_value::<ProjectsResponse>(v)?.projects)
    }

    pub async fn add_project(&self, path: &str) -> Result<AddProjectResponse> {
        let v = self
            .post(
                &format!("{}/api/projects", self.hub),
                json!({ "path": path }),
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    pub async fn project_inits(&self) -> Result<Vec<ProjectInit>> {
        let v = self.get(&format!("{}/api/projects/init", self.hub)).await?;
        Ok(serde_json::from_value::<ProjectInitsResponse>(v)?.inits)
    }

    pub async fn remove_project(&self, prefix: &str) -> Result<()> {
        self.delete(&format!("{}/api/projects/{prefix}", self.hub))
            .await?;
        Ok(())
    }

    /// Register `paths` into this server. Returns (migrated, failed).
    pub async fn migrate_projects(
        &self,
        paths: Vec<String>,
    ) -> Result<(Vec<ProjectSummary>, Vec<(String, String)>)> {
        let v = self
            .post(
                &format!("{}/api/projects/migrate", self.hub),
                json!({ "paths": paths }),
            )
            .await?;
        let migrated = v
            .get("migrated")
            .and_then(|m| serde_json::from_value::<Vec<ProjectSummary>>(m.clone()).ok())
            .unwrap_or_default();
        let failed = v
            .get("failed")
            .and_then(|f| f.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| {
                        let path = e.get("path")?.as_str()?.to_string();
                        let error = e
                            .get("error")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        Some((path, error))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok((migrated, failed))
    }

    /// Base URL for the project served at `project_dir`: `http://…/<prefix>`.
    /// Errors when the dir isn't a git repo or isn't a served project.
    pub async fn resolve_project_base(&self, project_dir: &str) -> Result<String> {
        let root = resolve_project_root(project_dir).ok_or_else(|| {
            anyhow!(
                "Not inside a git repository, so Sebenza can't tell which project this \
                 command targets. cd into a project it serves (`sebenza-cli project ls` lists them) \
                 and try again."
            )
        })?;
        let projects = self.fetch_projects().await?;
        let target = canonicalize_path(&root);
        let m = projects
            .into_iter()
            .find(|p| canonicalize_path(&p.path) == target)
            .ok_or_else(|| {
                anyhow!(
                    "This project ({root}) isn't served by Sebenza on port {}. Run \
                     `sebenza-cli project add` or start `sebenza-cli serve` in it first.",
                    self.port
                )
            })?;
        Ok(format!("{}/{}", self.hub, m.prefix))
    }

    // ── Project-scoped worktree operations ───────────────────────────────────

    pub async fn get_project(&self, base: &str) -> Result<ProjectSnapshot> {
        let v = self.get(&format!("{base}/api/project")).await?;
        Ok(serde_json::from_value(v)?)
    }

    /// Returns the list of created branches (primary first).
    pub async fn create_worktree(&self, base: &str, body: Value) -> Result<Vec<String>> {
        let v = self.post(&format!("{base}/api/worktrees"), body).await?;
        let parsed: CreateWorktreeResponse = serde_json::from_value(v)?;
        let mut branches = parsed.branches;
        if !branches.iter().any(|b| b == &parsed.primary_branch) {
            branches.insert(0, parsed.primary_branch);
        }
        Ok(branches)
    }

    pub async fn open_worktree(&self, base: &str, name: &str) -> Result<()> {
        self.open_worktree_body(base, name, json!({})).await
    }

    pub async fn open_worktree_body(&self, base: &str, name: &str, body: Value) -> Result<()> {
        self.post(&format!("{base}/api/worktrees/{name}/open"), body)
            .await?;
        Ok(())
    }

    /// Full worktree list (`GET /api/worktrees`), used by oneshot polling.
    pub async fn fetch_worktrees(&self, base: &str) -> Result<Vec<WorktreeSnapshot>> {
        let v = self.get(&format!("{base}/api/worktrees")).await?;
        Ok(serde_json::from_value::<WorktreeListResponse>(v)?.worktrees)
    }

    /// Create a worktree, returning the primary branch (first element).
    pub async fn create_worktree_primary(&self, base: &str, body: Value) -> Result<String> {
        let branches = self.create_worktree(base, body).await?;
        branches
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("server created no worktree"))
    }

    pub async fn sync_prs(&self, base: &str, name: &str) -> Result<()> {
        self.post(&format!("{base}/api/worktrees/{name}/sync-prs"), json!({}))
            .await?;
        Ok(())
    }

    /// Conversation history messages for a worktree agent.
    pub async fn history(&self, base: &str, name: &str) -> Result<Vec<AgentsUiMessage>> {
        let v = self
            .get(&format!("{base}/api/agents/worktrees/{name}/history"))
            .await?;
        Ok(serde_json::from_value::<ConversationHistoryResponse>(v)?
            .conversation
            .messages)
    }

    pub async fn close_worktree(&self, base: &str, name: &str) -> Result<()> {
        self.post(&format!("{base}/api/worktrees/{name}/close"), json!({}))
            .await?;
        Ok(())
    }

    pub async fn refresh_agent_terminal(&self, base: &str, name: &str) -> Result<()> {
        self.post(
            &format!("{base}/api/worktrees/{name}/agent-terminal/refresh"),
            json!({}),
        )
        .await?;
        Ok(())
    }

    pub async fn set_archived(&self, base: &str, name: &str, archived: bool) -> Result<()> {
        self.put(
            &format!("{base}/api/worktrees/{name}/archive"),
            json!({ "archived": archived }),
        )
        .await?;
        Ok(())
    }

    /// Returns the resulting label (None when cleared).
    pub async fn set_label(
        &self,
        base: &str,
        name: &str,
        label: Option<&str>,
    ) -> Result<Option<String>> {
        let v = self
            .put(
                &format!("{base}/api/worktrees/{name}/label"),
                json!({ "label": label }),
            )
            .await?;
        Ok(v.get("label").and_then(|l| l.as_str()).map(String::from))
    }

    pub async fn remove_worktree(&self, base: &str, name: &str) -> Result<()> {
        self.delete(&format!("{base}/api/worktrees/{name}")).await?;
        Ok(())
    }

    pub async fn merge_worktree(&self, base: &str, name: &str) -> Result<()> {
        self.post(&format!("{base}/api/worktrees/{name}/merge"), json!({}))
            .await?;
        Ok(())
    }

    pub async fn send_prompt(
        &self,
        base: &str,
        name: &str,
        text: &str,
        preamble: Option<&str>,
    ) -> Result<()> {
        let mut body = json!({ "text": text });
        if let Some(pre) = preamble {
            body["preamble"] = json!(pre);
        }
        self.post(&format!("{base}/api/worktrees/{name}/send"), body)
            .await?;
        Ok(())
    }

    /// Returns the created tab (id + label).
    pub async fn create_tab(&self, base: &str, name: &str) -> Result<WorktreeTab> {
        let v = self
            .post(&format!("{base}/api/worktrees/{name}/tabs"), json!({}))
            .await?;
        Ok(serde_json::from_value::<CreatedTab>(v)?.tab)
    }

    /// Start a fresh session of `agent` as a new tab, rather than forking.
    pub async fn create_agent_tab(
        &self,
        base: &str,
        name: &str,
        agent: &str,
    ) -> Result<WorktreeTab> {
        let v = self
            .post(
                &format!("{base}/api/worktrees/{name}/agent-tabs"),
                json!({ "agent": agent }),
            )
            .await?;
        Ok(serde_json::from_value::<CreatedTab>(v)?.tab)
    }

    pub async fn select_tab(&self, base: &str, name: &str, tab_id: &str) -> Result<()> {
        self.post(
            &format!("{base}/api/worktrees/{name}/tabs/{tab_id}/select"),
            json!({}),
        )
        .await?;
        Ok(())
    }

    pub async fn delete_tab(&self, base: &str, name: &str, tab_id: &str) -> Result<()> {
        self.delete(&format!("{base}/api/worktrees/{name}/tabs/{tab_id}"))
            .await?;
        Ok(())
    }
}
