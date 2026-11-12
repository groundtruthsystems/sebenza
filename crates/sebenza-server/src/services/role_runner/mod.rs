//! Software-factory role jobs (FR-12, FR-13, technical-architecture.md).
//!
//! A role job runs one agent CLI headless and read-only:
//! - in an empty per-item scratch directory, with an allowlisted environment
//!   and its own process group, killed on timeout;
//! - with the role's prompt embedded at build time from `skills/agents/*.md`;
//! - under a per-CLI read-only profile, enabled role by role only through the
//!   [`conformance`] table;
//! - returning one JSON envelope ([`output`]) that is validated, capped and
//!   scanned before anything is kept. The output is a proposal, never an act.
//!
//! Two lanes share `max_concurrent`: coordinator chat turns (180 s) and batch
//! design/review jobs (600 s). When `max_concurrent` ≥ 2 one slot is kept for
//! chat. Jobs for one item run one at a time, in arrival order.

pub mod output;

use crate::services::agent_stream::{
    AgentStreamManager, StartRunInput, StreamProvider, ToolPolicy,
};
use crate::services::system_agent::isolation::{child_env, prepare_scratch_dir};
use common::domain::solution::AgentProfile;
use output::{OutputError, RoleOutput, parse_role_output};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Semaphore;

/// A software-factory role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Coordinator,
    SystemAnalyst,
    SolutionsArchitect,
    SolutionReviewer,
    /// Reviews a project's design or code on a read-only worktree snapshot.
    ProjectReviewer,
}

/// Role prompts, embedded from the plugin so the binary is self-contained
/// (FR-13). TS-37 proves each equals its file.
pub const COORDINATOR_PROMPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../skills/agents/solution-coordinator.md"
));
pub const SYSTEM_ANALYST_PROMPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../skills/agents/system-analyst.md"
));
pub const SOLUTIONS_ARCHITECT_PROMPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../skills/agents/solutions-architect.md"
));
pub const SOLUTION_REVIEWER_PROMPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../skills/agents/solution-reviewer.md"
));
pub const PROJECT_REVIEWER_PROMPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../skills/agents/project-reviewer.md"
));

impl Role {
    pub const SOLUTION_ROLES: [Role; 4] = [
        Role::Coordinator,
        Role::SystemAnalyst,
        Role::SolutionsArchitect,
        Role::SolutionReviewer,
    ];
    pub const ALL: [Role; 5] = [
        Role::Coordinator,
        Role::SystemAnalyst,
        Role::SolutionsArchitect,
        Role::SolutionReviewer,
        Role::ProjectReviewer,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Coordinator => "solution-coordinator",
            Role::SystemAnalyst => "system-analyst",
            Role::SolutionsArchitect => "solutions-architect",
            Role::SolutionReviewer => "solution-reviewer",
            Role::ProjectReviewer => "project-reviewer",
        }
    }

    /// The embedded agent file, or `None` for a role whose prompt lives in
    /// another track.
    pub fn prompt(self) -> Option<&'static str> {
        match self {
            Role::Coordinator => Some(COORDINATOR_PROMPT),
            Role::SystemAnalyst => Some(SYSTEM_ANALYST_PROMPT),
            Role::SolutionsArchitect => Some(SOLUTIONS_ARCHITECT_PROMPT),
            Role::SolutionReviewer => Some(SOLUTION_REVIEWER_PROMPT),
            Role::ProjectReviewer => Some(PROJECT_REVIEWER_PROMPT),
        }
    }

    /// The prompt without its YAML frontmatter: the instructions an agent
    /// runs under. (A value starting `---` also reads as a flag to some CLIs.)
    pub fn instructions(self) -> Option<&'static str> {
        let p = self.prompt()?;
        Some(
            p.strip_prefix("---\n")
                .and_then(|rest| rest.split_once("\n---\n").map(|(_, body)| body))
                .unwrap_or(p)
                .trim_start(),
        )
    }

    pub fn lane(self) -> Lane {
        match self {
            Role::Coordinator => Lane::Chat,
            _ => Lane::Batch,
        }
    }

    pub fn is_reviewer(self) -> bool {
        matches!(self, Role::SolutionReviewer | Role::ProjectReviewer)
    }
}

/// Which pool a job draws from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Chat,
    Batch,
}

/// How far a CLI's read-only profile has been proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Enforcement {
    /// Its conformance test passed: a write or shell command had no effect.
    Verified,
    /// Not proven; shown as "read-only unverified" and needs the operator's
    /// acknowledgement. Never a reviewer.
    Unverified,
}

pub mod conformance {
    //! The code table that enables each CLI for role jobs. A row changes only
    //! when its opt-in conformance test (`role_runner::conformance_tests`)
    //! passes or fails against the real CLI.
    use super::Enforcement;

    /// (agent, enforcement, evidence).
    pub const TABLE: [(&str, Enforcement, &str); 3] = [
        (
            "claude",
            Enforcement::Verified,
            "plan mode + allow/deny tools + Read deny settings; TS-41/TS-61 conformance",
        ),
        (
            "codex",
            Enforcement::Verified,
            "exec --sandbox read-only --ephemeral; TS-41 conformance",
        ),
        (
            "grok",
            Enforcement::Unverified,
            "plan mode + --tools allowlist; sandbox profiles undocumented",
        ),
    ];

    pub fn enforcement(agent: &str) -> Option<Enforcement> {
        TABLE
            .iter()
            .find(|(a, _, _)| *a == agent)
            .map(|(_, e, _)| *e)
    }
}

/// Why a role job will not start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RoleRefusal {
    #[error("{0} cannot run role jobs")]
    UnknownAgent(String),
    #[error("{agent} cannot be the {role}: its read-only enforcement is unverified")]
    UnverifiedReviewer { agent: String, role: &'static str },
    #[error(
        "{0}'s read-only enforcement is unverified; the operator must acknowledge that before it runs"
    )]
    NeedsAcknowledgement(String),
    #[error("{0} is not installed (not found on PATH)")]
    NotInstalled(String),
    #[error("the {0} role has no prompt in this track")]
    NoPrompt(&'static str),
}

/// Whether `agent` may play `role`, and how far it is trusted.
pub fn role_allowed(
    agent: &str,
    role: Role,
    acknowledged: bool,
) -> Result<Enforcement, RoleRefusal> {
    let e = conformance::enforcement(agent)
        .ok_or_else(|| RoleRefusal::UnknownAgent(agent.to_string()))?;
    if e == Enforcement::Unverified {
        if role.is_reviewer() {
            return Err(RoleRefusal::UnverifiedReviewer {
                agent: agent.to_string(),
                role: role.as_str(),
            });
        }
        if !acknowledged {
            return Err(RoleRefusal::NeedsAcknowledgement(agent.to_string()));
        }
    }
    Ok(e)
}

/// `Read` deny rules for claude role jobs: Sebenza's state, SSH and cloud
/// credentials, other agents' homes (FT-06). TS-61 proves they hold.
pub const CLAUDE_READ_DENY: [&str; 10] = [
    "Read(~/.ai/**)",
    "Read(~/.ssh/**)",
    "Read(~/.aws/**)",
    "Read(~/.config/**)",
    "Read(~/.gnupg/**)",
    "Read(~/.netrc)",
    "Read(~/.docker/**)",
    "Read(~/.codex/**)",
    "Read(~/.grok/**)",
    "Read(~/.kube/**)",
];

/// Variables role children may inherit beyond the system agent's allowlist:
/// the codex and grok homes and keys.
pub const EXTRA_ENV: [&str; 6] = [
    "CODEX_HOME",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "XAI_API_KEY",
    "GROK_HOME",
    "GROK_API_KEY",
];

/// The read-only tool policy for `agent`.
pub fn tool_policy(agent: &str) -> ToolPolicy {
    match agent {
        "claude" => ToolPolicy {
            allowed: crate::services::system_agent::isolation::ALLOWED_TOOLS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            disallowed: crate::services::system_agent::isolation::DISALLOWED_TOOLS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            strict_mcp: true,
            // No hooks run in a role job: a project's or the user's hooks
            // are code that would execute inside the reviewer (FT-05).
            settings: Some(
                serde_json::json!({
                    "permissions": { "deny": CLAUDE_READ_DENY },
                    "disableAllHooks": true,
                })
                .to_string(),
            ),
            max_turns: None,
        },
        "grok" => ToolPolicy {
            allowed: ["read_file", "list_dir", "grep"].map(String::from).to_vec(),
            disallowed: [
                "run_terminal_command",
                "search_replace",
                "write",
                "spawn_subagent",
                "web_fetch",
                "web_search",
                "use_tool",
                "search_tool",
                "image_gen",
                "image_edit",
            ]
            .map(String::from)
            .to_vec(),
            strict_mcp: false,
            settings: None,
            max_turns: Some(12),
        },
        // codex has no tool list; `tools: Some(..)` switches it to the
        // read-only, ephemeral `exec` profile.
        _ => ToolPolicy::default(),
    }
}

pub fn provider(agent: &str) -> Option<StreamProvider> {
    match agent {
        "claude" => Some(StreamProvider::Claude),
        "codex" => Some(StreamProvider::Codex),
        "grok" => Some(StreamProvider::Grok),
        _ => None,
    }
}

/// One role job.
#[derive(Debug, Clone)]
pub struct RoleJob {
    pub item_id: String,
    pub role: Role,
    pub profile: AgentProfile,
    /// The per-turn prompt (fenced context plus the ask). The role prompt is
    /// added by the runner.
    pub prompt: String,
    /// The operator acknowledged running an unverified CLI.
    pub acknowledged_unverified: bool,
    /// Run here instead of an empty scratch directory (a review snapshot).
    pub cwd: Option<PathBuf>,
}

/// How a role job failed. None of these consume a review round.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RoleJobError {
    #[error(transparent)]
    Refused(#[from] RoleRefusal),
    #[error("the {0} job timed out")]
    Timeout(&'static str),
    #[error("the agent failed: {0}")]
    AgentFailed(String),
    #[error(transparent)]
    InvalidOutput(OutputError),
    #[error("the prompt is over the {MAX_PROMPT_BYTES}-byte limit")]
    PromptTooLarge,
}

impl RoleJobError {
    /// A stable code for job status (`failed: timeout`, ...).
    pub fn code(&self) -> &'static str {
        match self {
            RoleJobError::Refused(_) => "refused",
            RoleJobError::Timeout(_) => "timeout",
            RoleJobError::AgentFailed(_) => "agent_failed",
            RoleJobError::InvalidOutput(_) => "invalid_output",
            RoleJobError::PromptTooLarge => "too_large",
        }
    }
}

/// The argv-and-env launch for a job, built without running it.
pub fn build_launch(
    job: &RoleJob,
    cwd: &std::path::Path,
    timeout: Duration,
    binary: Option<String>,
    parent_env: impl IntoIterator<Item = (String, String)>,
) -> Result<StartRunInput, RoleJobError> {
    let provider = provider(&job.profile.agent)
        .ok_or_else(|| RoleRefusal::UnknownAgent(job.profile.agent.clone()))?;
    let system_prompt = job
        .role
        .instructions()
        .ok_or(RoleRefusal::NoPrompt(job.role.as_str()))?;
    let parent: Vec<(String, String)> = parent_env.into_iter().collect();
    let mut env = child_env(parent.clone());
    for (k, v) in parent {
        if EXTRA_ENV.contains(&k.as_str()) {
            env.insert(k, v);
        }
    }
    Ok(StartRunInput {
        provider,
        conversation_id: format!(
            "role:{}:{}:{}",
            job.item_id,
            job.role.as_str(),
            common::util::id::random_ulid()
        ),
        cwd: cwd.to_string_lossy().to_string(),
        prompt: job.prompt.clone(),
        env,
        permission_mode: (job.profile.agent != "codex").then(|| "plan".to_string()),
        resume_session_id: None,
        system_prompt: Some(system_prompt.to_string()),
        binary,
        model: job.profile.model.clone(),
        tools: Some(tool_policy(&job.profile.agent)),
        isolated: true,
        timeout: Some(timeout),
    })
}

/// The repair prompt for a reply that was not a valid envelope.
fn repair_prompt(job: &RoleJob, error: &OutputError) -> String {
    format!(
        "{}\n\nYour previous reply could not be used: {error}. Reply again with only the JSON object for role {}, and nothing else.",
        job.prompt,
        job.role.as_str()
    )
}

/// Runs role jobs. Cheap to share behind an `Arc`.
pub struct RoleRunner {
    stream: Arc<AgentStreamManager>,
    scratch_root: PathBuf,
    /// Every job holds one of these.
    all: Arc<Semaphore>,
    /// Batch jobs also hold one of these, so a chat slot stays free.
    batch: Arc<Semaphore>,
    items: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    chat_timeout: Duration,
    batch_timeout: Duration,
    /// Test override: agent -> binary.
    binaries: HashMap<String, String>,
    /// Exact strings to redact from output (control and operator tokens).
    known_secrets: Mutex<Vec<String>>,
    /// Test override for the `PATH` searched for installed CLIs.
    path: Option<std::ffi::OsString>,
}

/// Largest per-turn prompt a job accepts (FT-13).
pub const MAX_PROMPT_BYTES: usize = 128 * 1024;

pub const CHAT_TIMEOUT: Duration = Duration::from_secs(180);
pub const BATCH_TIMEOUT: Duration = Duration::from_secs(600);

impl RoleRunner {
    pub fn new(
        stream: Arc<AgentStreamManager>,
        scratch_root: PathBuf,
        max_concurrent: usize,
    ) -> Self {
        let max = max_concurrent.max(1);
        let batch = if max >= 2 { max - 1 } else { 1 };
        Self {
            stream,
            scratch_root,
            all: Arc::new(Semaphore::new(max)),
            batch: Arc::new(Semaphore::new(batch)),
            items: Mutex::new(HashMap::new()),
            chat_timeout: CHAT_TIMEOUT,
            batch_timeout: BATCH_TIMEOUT,
            binaries: HashMap::new(),
            known_secrets: Mutex::new(Vec::new()),
            path: None,
        }
    }

    pub fn with_search_path(mut self, path: &str) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_timeouts(mut self, chat: Duration, batch: Duration) -> Self {
        self.chat_timeout = chat;
        self.batch_timeout = batch;
        self
    }

    pub fn with_binary(mut self, agent: &str, binary: &str) -> Self {
        self.binaries.insert(agent.to_string(), binary.to_string());
        self
    }

    pub fn add_known_secret(&self, secret: &str) {
        let mut k = self.known_secrets.lock().unwrap_or_else(|e| e.into_inner());
        if !k.iter().any(|s| s == secret) {
            k.push(secret.to_string());
        }
    }

    fn installed(&self, agent: &str) -> bool {
        self.binaries.contains_key(agent)
            || self
                .path
                .clone()
                .or_else(|| std::env::var_os("PATH"))
                .is_some_and(|paths| std::env::split_paths(&paths).any(|p| p.join(agent).is_file()))
    }

    /// Run one job to a validated reply. Refusals come back before any slot is
    /// taken; a timeout, a failed agent or invalid output (after one repair
    /// retry) is an error and nothing is kept.
    pub async fn run(&self, job: RoleJob) -> Result<RoleOutput, RoleJobError> {
        if job.prompt.len() > MAX_PROMPT_BYTES {
            return Err(RoleJobError::PromptTooLarge);
        }
        role_allowed(&job.profile.agent, job.role, job.acknowledged_unverified)?;
        job.role
            .prompt()
            .ok_or(RoleRefusal::NoPrompt(job.role.as_str()))?;
        if !self.installed(&job.profile.agent) {
            return Err(RoleRefusal::NotInstalled(job.profile.agent.clone()).into());
        }
        // Per-item FIFO: tokio's mutex queues waiters fairly.
        let item_lock = {
            let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
            items.entry(job.item_id.clone()).or_default().clone()
        };
        let _item = item_lock.lock().await;
        let lane = job.role.lane();
        let _batch = match lane {
            Lane::Batch => Some(
                self.batch
                    .clone()
                    .acquire_owned()
                    .await
                    .expect("open semaphore"),
            ),
            Lane::Chat => None,
        };
        let _slot = self
            .all
            .clone()
            .acquire_owned()
            .await
            .expect("open semaphore");
        let timeout = match lane {
            Lane::Chat => self.chat_timeout,
            Lane::Batch => self.batch_timeout,
        };

        let mut attempt_job = job.clone();
        for attempt in 0..2 {
            let text = self.run_once(&attempt_job, timeout).await?;
            let known = self
                .known_secrets
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            match parse_role_output(job.role, &text, &known) {
                Ok(out) => return Ok(out),
                Err(e) if attempt == 0 && e.repairable() => {
                    attempt_job.prompt = repair_prompt(&job, &e);
                }
                Err(e) => return Err(RoleJobError::InvalidOutput(e)),
            }
        }
        unreachable!("the loop returns on its second attempt")
    }

    async fn run_once(&self, job: &RoleJob, timeout: Duration) -> Result<String, RoleJobError> {
        // A review snapshot when the job names one; otherwise an empty
        // per-item scratch directory.
        let scratch = match &job.cwd {
            Some(dir) => dir.clone(),
            None => prepare_scratch_dir(
                &self.scratch_root,
                &format!("{}{}", job.item_id, job.role.as_str().replace('-', "")),
            )
            .map_err(|e| RoleJobError::AgentFailed(format!("scratch dir: {e}")))?,
        };
        let launch = build_launch(
            job,
            &scratch,
            timeout,
            self.binaries.get(&job.profile.agent).cloned(),
            std::env::vars(),
        )?;
        let outcome = self
            .stream
            .run_to_completion(launch)
            .await
            .map_err(RoleJobError::AgentFailed)?;
        if outcome.timed_out {
            return Err(RoleJobError::Timeout(job.role.as_str()));
        }
        if let Some(err) = outcome.error {
            return Err(RoleJobError::AgentFailed(err));
        }
        if outcome.exit_code != Some(0) {
            return Err(RoleJobError::AgentFailed(format!(
                "exited with {:?}",
                outcome.exit_code
            )));
        }
        outcome
            .final_message
            .ok_or_else(|| RoleJobError::AgentFailed("no final message".into()))
    }
}

#[cfg(test)]
mod conformance_tests;
/// A runner whose every CLI is the checked-in stub, for tests outside this
/// module. Short timeouts; never a real CLI.
#[cfg(test)]
pub fn test_runner(base: &std::path::Path) -> RoleRunner {
    let stub = format!(
        "{}/src/services/role_runner/testdata/stub-role.sh",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut r = RoleRunner::new(Arc::new(AgentStreamManager::new()), base.join("roles"), 2)
        .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
        .with_search_path("/nonexistent");
    for agent in ["claude", "codex", "grok"] {
        r = r.with_binary(agent, &stub);
    }
    r
}

#[cfg(test)]
mod tests;
