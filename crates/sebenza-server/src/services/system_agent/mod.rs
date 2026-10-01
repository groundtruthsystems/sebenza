//! The inbox system agent: one resumable headless claude session per item,
//! run as short-lived jobs (triage, draft-help, convert).
//!
//! Jobs for one item run strictly one at a time, in FIFO order, because
//! `AgentStreamManager::start_run` refuses a second turn on a conversation
//! (AA-D1). Across items at most `maxConcurrent` children run, and draft-help
//! and convert jobs (someone is waiting) jump ahead of triage (TD-5). Each
//! child is isolated (see [`isolation`]) and killed, with its whole process
//! group, at `timeoutSecs` (TD-6). The agent only proposes: a job ends in a
//! validated, typed [`JobOutput`] handed to a [`JobResultSink`]; applying it
//! (priority, proposal, advice, flags) is the sink's business (AA-D2).

pub mod apply;
pub mod isolation;
pub mod output;
pub mod prompt;

#[allow(unused_imports)]
pub use output::{
    ConvertOutput, ConvertTargetOutput, DraftHelpOutput, JobOutput, OutputError, Recommendation,
    TriageOutput,
};

use crate::domain::config::SystemAgentConfig;
use crate::domain::inbox_events::{AgentSession, InboxEvent, RequestStatus};
use crate::domain::model::{InboxDraft, InboxDraftView};
use crate::services::agent_stream::AgentStreamManager;
use crate::services::inbox_service::{InboxService, ListQuery, RequestObserver};
use crate::util::id::random_ulid;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock, Weak};

/// What a job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Triage,
    DraftHelp,
    Convert,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            JobKind::Triage => "triage",
            JobKind::DraftHelp => "draft_help",
            JobKind::Convert => "convert",
        }
    }

    /// Someone is waiting on the answer, so it runs ahead of triage (TD-5).
    pub fn is_interactive(self) -> bool {
        matches!(self, JobKind::DraftHelp | JobKind::Convert)
    }
}

/// A job's parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobInput {
    /// Rank a request and recommend a resolution. `attempt` starts at 1 and
    /// with `request_id` dedupes the job (TD-2).
    Triage { request_id: String, attempt: u32 },
    /// Propose a new body for the draft, optionally steered by the operator.
    DraftHelp { instruction: Option<String> },
    /// Formulate a `system_instruction` for each target project.
    Convert {
        targets: Vec<String>,
        operator_prompt: Option<String>,
    },
}

impl JobInput {
    pub fn kind(&self) -> JobKind {
        match self {
            JobInput::Triage { .. } => JobKind::Triage,
            JobInput::DraftHelp { .. } => JobKind::DraftHelp,
            JobInput::Convert { .. } => JobKind::Convert,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
}

impl JobStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, JobStatus::Succeeded | JobStatus::Failed)
    }
}

/// One job, as the registry (and phase 4's job-status route) reports it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    pub job_id: String,
    pub draft_id: String,
    pub kind: JobKind,
    pub request_id: Option<String>,
    pub attempt: u32,
    pub status: JobStatus,
    /// Set when the job succeeded.
    pub output: Option<JobOutput>,
    /// Set when the job failed. Never contains the agent's reply.
    pub error: Option<String>,
    /// The session was new or past its turn cap and was re-seeded.
    pub reseeded: bool,
    pub enqueued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    #[serde(skip)]
    pub input: JobInput,
}

/// Told when a job finishes, successfully or not. Phase 4 applies results
/// here: priority, proposal or advice events, and the triage-failed flag.
pub trait JobResultSink: Send + Sync {
    fn job_finished(&self, job: &JobRecord);
}

/// Logs each finished job, metadata only. The default sink.
pub struct LoggingJobSink;

impl JobResultSink for LoggingJobSink {
    fn job_finished(&self, job: &JobRecord) {
        tracing::info!(
            job_id = %job.job_id,
            draft_id = %job.draft_id,
            kind = job.kind.as_str(),
            request_id = job.request_id.as_deref().unwrap_or(""),
            status = ?job.status,
            "system agent job finished"
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueError {
    /// `systemAgent.enabled` is false (FR-22).
    Disabled,
    /// No async runtime to run the job on.
    NoRuntime,
    /// The item already has as many jobs waiting as it may (TA-R1).
    QueueFull(String),
    Invalid(String),
    /// No such request on the item.
    UnknownRequest(String),
    /// The request is past triage (proposed, confirmed or resolved).
    NotRetryable(String),
}

impl std::fmt::Display for EnqueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnqueueError::Disabled => write!(f, "the system agent is disabled"),
            EnqueueError::NoRuntime => write!(f, "the system agent is not running"),
            EnqueueError::QueueFull(id) => {
                write!(f, "too many system agent jobs are queued for draft {id}")
            }
            EnqueueError::Invalid(e) => write!(f, "{e}"),
            EnqueueError::UnknownRequest(id) => write!(f, "unknown request {id}"),
            EnqueueError::NotRetryable(status) => {
                write!(
                    f,
                    "request is {status}; only an open request can be re-triaged"
                )
            }
        }
    }
}

impl std::error::Error for EnqueueError {}

/// Jobs one item may have waiting before more are refused (TA-R1).
pub const MAX_QUEUED_PER_ITEM: usize = 32;
/// Finished jobs the registry remembers.
pub const FINISHED_JOBS_KEPT: usize = 512;

/// Where and with what parent environment children start.
#[derive(Debug, Clone)]
pub struct SystemAgentOptions {
    /// Per-item scratch directories live under here.
    pub scratch_root: PathBuf,
    /// The environment [`isolation::child_env`] filters. `None` is the
    /// daemon's own.
    pub parent_env: Option<Vec<(String, String)>>,
}

impl Default for SystemAgentOptions {
    fn default() -> Self {
        Self {
            scratch_root: std::env::temp_dir().join("sebenza-system-agent"),
            parent_env: None,
        }
    }
}

/// Runs system-agent jobs. Cheap to share: hold it in an `Arc`.
pub struct SystemAgentService {
    config: SystemAgentConfig,
    inbox: Arc<InboxService>,
    stream: Arc<AgentStreamManager>,
    options: SystemAgentOptions,
    sink: RwLock<Arc<dyn JobResultSink>>,
    /// Where jobs run; `None` when built outside a tokio runtime.
    runtime: Option<tokio::runtime::Handle>,
    /// Every queue decision is made under this one lock, so a check (is the
    /// item busy? is there a free slot?) and the claim that follows it can
    /// never interleave with another enqueue (TA-R2).
    state: Mutex<QueueState>,
    /// Bumped whenever a job finishes; `wait` watches it.
    finished: tokio::sync::watch::Sender<u64>,
    /// Every job state change (queued, running, finished), for the
    /// `inbox.job` WebSocket event (FR-20).
    updates: tokio::sync::broadcast::Sender<JobRecord>,
    this: Weak<Self>,
}

#[derive(Default)]
struct QueueState {
    seq: u64,
    /// Waiting jobs by `(lane, seq)`: lane 0 (interactive) before lane 1
    /// (triage), FIFO within a lane.
    queued: BTreeMap<(u8, u64), String>,
    /// Items with a job running right now.
    busy: HashSet<String>,
    running: usize,
    jobs: HashMap<String, (u64, JobRecord)>,
    /// Finished job ids, oldest first, for bounded retention.
    done: VecDeque<String>,
    /// `(draft_id, request_id, kind, attempt)` to job id (TD-2).
    dedupe: HashMap<(String, String, JobKind, u32), String>,
}

/// A failed job: why, and whether its session had been re-seeded.
struct Failure {
    error: String,
    reseeded: bool,
}

impl Failure {
    fn new(error: impl Into<String>, reseeded: bool) -> Self {
        Self {
            error: error.into(),
            reseeded,
        }
    }
}

/// Everything a job reads from the inbox, loaded off the async runtime.
struct JobContext {
    draft: InboxDraft,
    events: Vec<InboxEvent>,
    session: Option<AgentSession>,
    digest: Vec<prompt::DigestEntry>,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl SystemAgentService {
    pub fn new(
        config: SystemAgentConfig,
        inbox: Arc<InboxService>,
        stream: Arc<AgentStreamManager>,
        options: SystemAgentOptions,
    ) -> Arc<Self> {
        Arc::new_cyclic(|this| Self {
            config,
            inbox,
            stream,
            options,
            sink: RwLock::new(Arc::new(LoggingJobSink)),
            runtime: tokio::runtime::Handle::try_current().ok(),
            state: Mutex::new(QueueState::default()),
            finished: tokio::sync::watch::channel(0).0,
            updates: tokio::sync::broadcast::channel(256).0,
            this: this.clone(),
        })
    }

    pub fn config(&self) -> &SystemAgentConfig {
        &self.config
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Replace where finished jobs go.
    pub fn set_sink(&self, sink: Arc<dyn JobResultSink>) {
        *self.sink.write().unwrap_or_else(|e| e.into_inner()) = sink;
    }

    /// The observer to register on [`InboxService`]: each new request
    /// enqueues a triage. Holds the service weakly, so the inbox and the
    /// service do not keep each other alive.
    pub fn observer(self: &Arc<Self>) -> Arc<dyn RequestObserver> {
        Arc::new(TriageOnRequest(Arc::downgrade(self)))
    }

    /// Queue a job and return its id. A triage already known for the same
    /// `(request_id, attempt)` returns the existing job's id instead (TD-2).
    pub fn enqueue(&self, draft_id: &str, input: JobInput) -> Result<String, EnqueueError> {
        self.enqueue_new(draft_id, input).map(|(id, _)| id)
    }

    /// [`Self::enqueue`], also saying whether the job is new (not a dedupe hit).
    fn enqueue_new(&self, draft_id: &str, input: JobInput) -> Result<(String, bool), EnqueueError> {
        if !self.config.enabled {
            return Err(EnqueueError::Disabled);
        }
        let runtime = self.runtime.as_ref().ok_or(EnqueueError::NoRuntime)?;
        match &input {
            JobInput::Triage {
                request_id,
                attempt,
            } => {
                if request_id.trim().is_empty() || *attempt == 0 {
                    return Err(EnqueueError::Invalid(
                        "a triage needs a request id and an attempt of 1 or more".to_string(),
                    ));
                }
            }
            JobInput::Convert { targets, .. } if targets.is_empty() => {
                return Err(EnqueueError::Invalid(
                    "a convert job needs at least one target".to_string(),
                ));
            }
            _ => {}
        }
        match self.inbox.get(draft_id) {
            Ok((InboxDraftView::Parsed(_), _)) => {}
            _ => return Err(EnqueueError::Invalid(format!("unknown draft {draft_id}"))),
        }

        let kind = input.kind();
        let (request_id, attempt) = match &input {
            JobInput::Triage {
                request_id,
                attempt,
            } => (Some(request_id.clone()), *attempt),
            _ => (None, 1),
        };
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let key = request_id
            .clone()
            .map(|r| (draft_id.to_string(), r, kind, attempt));
        if let Some(existing) = key.as_ref().and_then(|k| st.dedupe.get(k)) {
            return Ok((existing.clone(), false));
        }
        let waiting = st
            .queued
            .values()
            .filter(|id| {
                st.jobs
                    .get(*id)
                    .is_some_and(|(_, j)| j.draft_id == draft_id)
            })
            .count();
        if waiting >= MAX_QUEUED_PER_ITEM {
            return Err(EnqueueError::QueueFull(draft_id.to_string()));
        }
        st.seq += 1;
        let seq = st.seq;
        let job_id = random_ulid();
        let record = JobRecord {
            job_id: job_id.clone(),
            draft_id: draft_id.to_string(),
            kind,
            request_id,
            attempt,
            status: JobStatus::Queued,
            output: None,
            error: None,
            reseeded: false,
            enqueued_at: now(),
            started_at: None,
            finished_at: None,
            input,
        };
        st.jobs.insert(job_id.clone(), (seq, record));
        let lane = if kind.is_interactive() { 0 } else { 1 };
        st.queued.insert((lane, seq), job_id.clone());
        if let Some(key) = key {
            st.dedupe.insert(key, job_id.clone());
        }
        tracing::info!(
            job_id = %job_id,
            draft_id,
            kind = kind.as_str(),
            queue_depth = st.queued.len(),
            "system agent job queued"
        );
        self.dispatch(&mut st, runtime);
        Ok((job_id, true))
    }

    /// Start every waiting job that may run now: the oldest in the best lane
    /// whose item is idle, while a global slot is free.
    fn dispatch(&self, st: &mut QueueState, runtime: &tokio::runtime::Handle) {
        let Some(this) = self.this.upgrade() else {
            return;
        };
        while st.running < self.config.max_concurrent.max(1) {
            let next = st
                .queued
                .iter()
                .find(|(_, id)| {
                    st.jobs
                        .get(*id)
                        .is_some_and(|(_, j)| !st.busy.contains(&j.draft_id))
                })
                .map(|(key, id)| (*key, id.clone()));
            let Some((key, job_id)) = next else {
                break;
            };
            st.queued.remove(&key);
            let Some((_, record)) = st.jobs.get_mut(&job_id) else {
                continue;
            };
            record.status = JobStatus::Running;
            record.started_at = Some(now());
            let draft_id = record.draft_id.clone();
            st.busy.insert(draft_id);
            st.running += 1;
            let this = this.clone();
            runtime.spawn(async move { this.run(job_id).await });
        }
    }

    /// Run one claimed job to its end, record it, free its slot, and tell
    /// the sink and any waiters.
    async fn run(self: Arc<Self>, job_id: String) {
        let Some(record) = self.job(&job_id) else {
            return;
        };
        let started = std::time::Instant::now();
        // Run on its own task so a panic fails the job instead of leaving its
        // item marked busy forever.
        let result = match tokio::spawn(self.clone().execute(record.clone())).await {
            Ok(result) => result,
            Err(_) => Err(Failure::new("the job panicked", false)),
        };
        let finished = {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.busy.remove(&record.draft_id);
            st.running = st.running.saturating_sub(1);
            let snapshot = st.jobs.get_mut(&job_id).map(|(_, job)| {
                match result {
                    Ok((output, reseeded)) => {
                        job.status = JobStatus::Succeeded;
                        job.output = Some(output);
                        job.reseeded = reseeded;
                    }
                    Err(failure) => {
                        job.status = JobStatus::Failed;
                        job.error = Some(failure.error);
                        job.reseeded = failure.reseeded;
                    }
                }
                job.finished_at = Some(now());
                job.clone()
            });
            st.done.push_back(job_id.clone());
            while st.done.len() > FINISHED_JOBS_KEPT {
                if let Some(old) = st.done.pop_front() {
                    st.jobs.remove(&old);
                }
            }
            if let Some(runtime) = &self.runtime {
                self.dispatch(&mut st, runtime);
            }
            snapshot
        };
        if let Some(job) = finished {
            tracing::info!(
                job_id = %job.job_id,
                draft_id = %job.draft_id,
                kind = job.kind.as_str(),
                status = ?job.status,
                duration_ms = started.elapsed().as_millis() as u64,
                timed_out = job.error.as_deref().is_some_and(|e| e.contains("timed out")),
                "system agent job ran"
            );
            let sink = self.sink.read().unwrap_or_else(|e| e.into_inner()).clone();
            sink.job_finished(&job);
        }
        self.finished.send_modify(|n| *n += 1);
    }

    /// Load the item, build the prompt, run the isolated child, and validate
    /// what it said. Writes nothing but the session, and only on success.
    async fn execute(self: Arc<Self>, record: JobRecord) -> Result<(JobOutput, bool), Failure> {
        let inbox = self.inbox.clone();
        let draft_id = record.draft_id.clone();
        let kind = record.kind;
        let ctx = tokio::task::spawn_blocking(move || load_context(&inbox, &draft_id, kind))
            .await
            .map_err(|_| Failure::new("loading the item failed", false))?
            .map_err(|e| Failure::new(e, false))?;

        let reseed = match &ctx.session {
            None => true,
            Some(s) => {
                s.turns >= self.config.turn_cap
                    || s.agent != self.config.agent.as_str()
                    || s.session_id.trim().is_empty()
            }
        };
        let prompt = prompt::build_job_prompt(
            &prompt::PromptContext {
                draft: &ctx.draft,
                events: &ctx.events,
                digest: &ctx.digest,
                reseed,
            },
            &record.input,
        )
        .map_err(|e| Failure::new(e, reseed))?;
        let cwd = isolation::prepare_scratch_dir(&self.options.scratch_root, &record.draft_id)
            .map_err(|e| Failure::new(format!("cannot prepare the scratch dir: {e}"), reseed))?;
        let parent = self
            .options
            .parent_env
            .clone()
            .unwrap_or_else(|| std::env::vars().collect());
        let resume = (!reseed)
            .then(|| ctx.session.as_ref().map(|s| s.session_id.clone()))
            .flatten();
        let input = isolation::build_run_input(
            &self.config,
            &record.draft_id,
            &cwd,
            prompt,
            resume.clone(),
            isolation::child_env(parent),
        );
        let outcome = self
            .stream
            .run_to_completion(input)
            .await
            .map_err(|e| Failure::new(e, reseed))?;

        if outcome.timed_out {
            return Err(Failure::new(
                format!("the agent timed out after {}s", self.config.timeout_secs),
                reseed,
            ));
        }
        if let Some(e) = &outcome.error {
            let short: String = e.chars().take(200).collect();
            return Err(Failure::new(
                format!("the agent run failed: {short}"),
                reseed,
            ));
        }
        match outcome.exit_code {
            Some(0) => {}
            Some(code) => {
                return Err(Failure::new(
                    format!("the agent exited with status {code}"),
                    reseed,
                ));
            }
            None => return Err(Failure::new("the agent was killed", reseed)),
        }
        let message = outcome
            .final_message
            .ok_or_else(|| Failure::new("the agent produced no final message", reseed))?;
        let expected = match &record.input {
            JobInput::Convert { targets, .. } => targets.clone(),
            _ => Vec::new(),
        };
        let output = output::parse_job_output(kind, &message, &expected)
            .map_err(|e| Failure::new(e.to_string(), reseed))?;

        if let Some(session_id) = outcome.session_id.or(resume) {
            let turns = match (&ctx.session, reseed) {
                (Some(s), false) => s.turns.saturating_add(1),
                _ => 1,
            };
            let session = AgentSession {
                agent: self.config.agent.as_str().to_string(),
                model: self.config.model.clone(),
                session_id,
                turns,
            };
            let inbox = self.inbox.clone();
            let draft_id = record.draft_id.clone();
            let written =
                tokio::task::spawn_blocking(move || inbox.write_session(&draft_id, &session)).await;
            if !matches!(written, Ok(Ok(()))) {
                tracing::warn!(draft_id = %record.draft_id, "system agent: session not saved");
            }
        }
        Ok((output, reseed))
    }

    /// Follow every job state change. Lagging receivers miss updates; the
    /// job-status route is authoritative.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<JobRecord> {
        self.updates.subscribe()
    }

    /// Tell subscribers a job changed. No subscriber is not an error.
    fn publish(&self, job: &JobRecord) {
        let _ = job;
    }

    /// Re-run triage for an open request as its next attempt (UC-05a, TS-63):
    /// one more than any attempt this run has seen for it, or than the
    /// triage outcomes its log records, so the dedupe key is fresh.
    pub fn retry_triage(&self, draft_id: &str, request_id: &str) -> Result<String, EnqueueError> {
        use crate::domain::inbox_events::InboxEventKind;
        use crate::services::inbox_service::InboxServiceError;
        if !self.config.enabled {
            return Err(EnqueueError::Disabled);
        }
        let events = self.inbox.events(draft_id).map_err(|e| match e {
            InboxServiceError::Store(_) => {
                EnqueueError::Invalid(format!("unknown draft {draft_id}"))
            }
            other => EnqueueError::Invalid(other.to_string()),
        })?;
        let request = crate::domain::inbox_events::fold_requests(&events)
            .into_iter()
            .find(|r| r.request_id == request_id)
            .ok_or_else(|| EnqueueError::UnknownRequest(request_id.to_string()))?;
        if request.status != RequestStatus::Open {
            let status = serde_json::to_value(request.status)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            return Err(EnqueueError::NotRetryable(status));
        }
        let (in_flight, seen) = {
            let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let mine: Vec<&JobRecord> = st
                .jobs
                .values()
                .map(|(_, j)| j)
                .filter(|j| {
                    j.draft_id == draft_id
                        && j.kind == JobKind::Triage
                        && j.request_id.as_deref() == Some(request_id)
                })
                .collect();
            (
                mine.iter()
                    .find(|j| !j.status.is_terminal())
                    .map(|j| j.job_id.clone()),
                mine.iter().map(|j| j.attempt).max().unwrap_or(0),
            )
        };
        // A triage already waiting or running is the retry.
        if let Some(job_id) = in_flight {
            return Ok(job_id);
        }
        // After a restart the registry is empty; the log still counts the
        // attempts that ended (failed, proposed or advised).
        let logged = events
            .iter()
            .filter(|e| match &e.kind {
                InboxEventKind::TriageFailed { request_id: r, .. }
                | InboxEventKind::Proposal { request_id: r, .. }
                | InboxEventKind::Advice { request_id: r, .. } => r == request_id,
                _ => false,
            })
            .count() as u32;
        let attempt = seen.max(logged) + 1;
        tracing::info!(draft_id, request_id, attempt, "triage retry requested");
        self.enqueue(
            draft_id,
            JobInput::Triage {
                request_id: request_id.to_string(),
                attempt,
            },
        )
    }

    /// One job by id.
    pub fn job(&self, job_id: &str) -> Option<JobRecord> {
        let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.jobs.get(job_id).map(|(_, job)| job.clone())
    }

    /// An item's known jobs, oldest first.
    pub fn jobs_for(&self, draft_id: &str) -> Vec<JobRecord> {
        let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut jobs: Vec<&(u64, JobRecord)> = st
            .jobs
            .values()
            .filter(|(_, j)| j.draft_id == draft_id)
            .collect();
        jobs.sort_by_key(|(seq, _)| *seq);
        jobs.into_iter().map(|(_, j)| j.clone()).collect()
    }

    /// Jobs waiting for a slot, across every item.
    pub fn queue_depth(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .queued
            .len()
    }

    /// Wait for a job to finish. `None` for an unknown id.
    pub async fn wait(&self, job_id: &str) -> Option<JobRecord> {
        // Subscribe before looking, so a finish between the two is not missed.
        let mut finished = self.finished.subscribe();
        loop {
            let job = self.job(job_id)?;
            if job.status.is_terminal() {
                return Some(job);
            }
            if finished.changed().await.is_err() {
                return self.job(job_id);
            }
        }
    }

    /// Re-enqueue, once, every open request on a live item that was never
    /// triaged: no proposal, not flagged, not rejected. Returns how many jobs
    /// were queued (TD-2, FR-21).
    pub fn recover_on_startup(&self) -> usize {
        if !self.config.enabled {
            return 0;
        }
        let items = match self.inbox.list(&ListQuery::default()) {
            Ok(items) => items,
            Err(e) => {
                tracing::warn!("system agent recovery: cannot list the inbox: {e}");
                return 0;
            }
        };
        let mut queued = 0;
        for item in items.iter().filter(|i| !i.is_raw) {
            let Ok(requests) = self.inbox.list_requests(&item.id) else {
                continue;
            };
            let untriaged = requests.into_iter().filter(|r| {
                r.status == RequestStatus::Open
                    && r.proposal_id.is_none()
                    && !r.flagged
                    && r.last_reason.is_none()
            });
            for request in untriaged {
                let input = JobInput::Triage {
                    request_id: request.request_id,
                    attempt: 1,
                };
                match self.enqueue_new(&item.id, input) {
                    Ok((_, true)) => queued += 1,
                    Ok((_, false)) => {}
                    Err(e) => {
                        tracing::warn!(draft_id = %item.id, "recovery triage not queued: {e}")
                    }
                }
            }
        }
        queued
    }
}

/// Load what a `kind` job needs. Blocking: reads the store.
fn load_context(inbox: &InboxService, draft_id: &str, kind: JobKind) -> Result<JobContext, String> {
    let draft = match inbox.get(draft_id) {
        Ok((InboxDraftView::Parsed(draft), _)) => draft,
        Ok(_) => return Err("the item does not parse".to_string()),
        Err(e) => return Err(format!("cannot read the item: {e}")),
    };
    let events = inbox
        .events(draft_id)
        .map_err(|e| format!("cannot read the item's events: {e}"))?;
    let session = inbox
        .read_session(draft_id)
        .map_err(|e| format!("cannot read the item's session: {e}"))?;
    let digest = if kind == JobKind::Triage {
        let items = inbox
            .list(&ListQuery::default())
            .map_err(|e| format!("cannot list the inbox: {e}"))?;
        prompt::build_digest(&items, draft_id)
    } else {
        Vec::new()
    };
    Ok(JobContext {
        draft,
        events,
        session,
        digest,
    })
}

/// Enqueues a triage for each new request.
struct TriageOnRequest(Weak<SystemAgentService>);

impl RequestObserver for TriageOnRequest {
    fn request_opened(&self, draft_id: &str, request_id: &str) {
        let Some(service) = self.0.upgrade() else {
            return;
        };
        let input = JobInput::Triage {
            request_id: request_id.to_string(),
            attempt: 1,
        };
        match service.enqueue(draft_id, input) {
            Ok(_) | Err(EnqueueError::Disabled) => {}
            Err(e) => tracing::warn!(draft_id, request_id, "triage not queued: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::inbox_store::{EventAuthor, InboxStore};
    use crate::adapters::projects_registry::ProjectsRegistry;
    use crate::domain::inbox_events::{InboxEventKind, WorktreeKey};
    use crate::domain::model::{DraftStatus, Priority};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    const FIXTURE_SESSION: &str = "5e55a9e7-1b2c-4d3e-8f90-0a1b2c3d4e5f";

    #[derive(Default)]
    struct Recorded(Mutex<Vec<JobRecord>>);
    impl JobResultSink for Recorded {
        fn job_finished(&self, job: &JobRecord) {
            self.0.lock().unwrap().push(job.clone());
        }
    }

    struct Harness {
        svc: Arc<SystemAgentService>,
        inbox: Arc<InboxService>,
        store: InboxStore,
        stub_dir: PathBuf,
        scratch: PathBuf,
        sink: Arc<Recorded>,
    }

    fn testdata() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/services/system_agent/testdata")
    }

    /// A tempdir inbox and a private copy of the stub agent in `mode`.
    fn harness(mode: &str, tweak: impl FnOnce(&mut SystemAgentConfig)) -> Harness {
        use std::os::unix::fs::PermissionsExt;
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("sebenza-sa-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let stub_dir = base.join("stub");
        std::fs::create_dir_all(stub_dir.join("fixtures")).unwrap();
        let script = stub_dir.join("stub-agent.sh");
        std::fs::copy(testdata().join("stub-agent.sh"), &script).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        for entry in std::fs::read_dir(testdata().join("fixtures")).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(
                entry.path(),
                stub_dir.join("fixtures").join(entry.file_name()),
            )
            .unwrap();
        }
        std::fs::write(stub_dir.join("stub.mode"), mode).unwrap();

        let mut config = SystemAgentConfig {
            enabled: true,
            timeout_secs: 20,
            binary: Some(script.to_string_lossy().to_string()),
            ..SystemAgentConfig::default()
        };
        tweak(&mut config);

        let store = InboxStore::with_dir(base.join("inbox"));
        let inbox = Arc::new(InboxService::new(
            InboxStore::with_dir(base.join("inbox")),
            ProjectsRegistry::with_file(base.join("projects.json")),
        ));
        let scratch = base.join("scratch");
        let options = SystemAgentOptions {
            scratch_root: scratch.clone(),
            parent_env: Some(vec![
                ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
                (
                    "HOME".into(),
                    base.join("home").to_string_lossy().to_string(),
                ),
                ("SEBENZA_CONTROL_TOKEN".into(), "must-not-leak".into()),
                ("GITHUB_TOKEN".into(), "ghp_TEST000".into()),
            ]),
        };
        let svc = SystemAgentService::new(
            config,
            inbox.clone(),
            Arc::new(AgentStreamManager::new()),
            options,
        );
        let sink = Arc::new(Recorded::default());
        svc.set_sink(sink.clone());
        Harness {
            svc,
            inbox,
            store,
            stub_dir,
            scratch,
            sink,
        }
    }

    impl Harness {
        fn item(&self, title: &str) -> String {
            self.inbox.create(title).expect("create").id
        }

        /// Append a request straight to the log (no observer, no conversion check).
        fn request(&self, draft: &str, request_id: &str) {
            self.store
                .append_event(
                    draft,
                    EventAuthor::worktree_agent(),
                    None,
                    InboxEventKind::RequestOpened {
                        request_id: request_id.into(),
                        worktree: WorktreeKey {
                            project: "acme-demo".into(),
                            branch: "feat/importer".into(),
                        },
                        title: format!("Request {request_id}"),
                        body: "Where is the fixture loader?".into(),
                        warnings: vec![],
                    },
                )
                .expect("append");
        }

        fn triage(&self, draft: &str, request_id: &str) -> String {
            self.svc
                .enqueue(
                    draft,
                    JobInput::Triage {
                        request_id: request_id.into(),
                        attempt: 1,
                    },
                )
                .expect("enqueue")
        }

        async fn finish(&self, job_id: &str) -> JobRecord {
            tokio::time::timeout(Duration::from_secs(30), self.svc.wait(job_id))
                .await
                .expect("job finished in time")
                .expect("known job")
        }

        fn log(&self) -> PathBuf {
            self.stub_dir.join("log")
        }

        fn logged(&self, prefix: &str) -> Vec<String> {
            let Ok(dir) = std::fs::read_dir(self.log()) else {
                return vec![];
            };
            let mut files: Vec<PathBuf> = dir
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with(&format!("{prefix}.")))
                })
                .collect();
            files.sort();
            files
                .iter()
                .map(|p| std::fs::read_to_string(p).unwrap())
                .collect()
        }

        fn spawns(&self) -> usize {
            self.logged("argv").len()
        }

        fn peak(&self) -> usize {
            std::fs::read_to_string(self.log().join("peaks"))
                .unwrap_or_default()
                .lines()
                .filter_map(|l| l.trim().parse().ok())
                .max()
                .unwrap_or(0)
        }

        fn order(&self) -> Vec<String> {
            std::fs::read_to_string(self.log().join("order"))
                .unwrap_or_default()
                .lines()
                .filter_map(|l| l.split_whitespace().next().map(str::to_string))
                .collect()
        }
    }

    fn argv_has(argv: &str, flag: &str) -> bool {
        argv.lines().any(|l| l == flag)
    }

    // TS-16 / TS-51: a triage on an item with no session produces a validated
    // typed result, seeds the session, and persists the CLI's session id.
    #[tokio::test]
    async fn a_triage_job_yields_a_typed_result_and_seeds_the_session() {
        let h = harness("ok", |_| {});
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        let job = h.finish(&h.triage(&draft, "R1")).await;

        assert_eq!(job.status, JobStatus::Succeeded, "{:?}", job.error);
        assert!(job.reseeded);
        let Some(JobOutput::Triage(t)) = &job.output else {
            panic!("triage output: {job:?}");
        };
        assert_eq!(t.priority, Priority::P1);
        assert_eq!(t.recommendation, Recommendation::Proposal);

        let session = h
            .store
            .read_session(&draft)
            .unwrap()
            .expect("session written");
        assert_eq!(session.session_id, FIXTURE_SESSION);
        assert_eq!(session.agent, "claude");
        assert_eq!(session.turns, 1);

        let prompts = h.logged("prompt");
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("SESSION-SEED"));
        assert!(!argv_has(&h.logged("argv")[0], "-r"));

        let seen = h.sink.0.lock().unwrap().clone();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].job_id, job.job_id);
        assert_eq!(h.svc.job(&job.job_id).unwrap().status, JobStatus::Succeeded);
    }

    // TS-51: under the turn cap the session is resumed, not re-seeded.
    #[tokio::test]
    async fn a_live_session_is_resumed() {
        let h = harness("ok", |c| c.model = Some("claude-haiku-4-5".into()));
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        let prior = AgentSession {
            agent: "claude".into(),
            model: None,
            session_id: "prior-session".into(),
            turns: 3,
        };
        h.store.write_session(&draft, &prior).unwrap();

        let job = h.finish(&h.triage(&draft, "R1")).await;
        assert_eq!(job.status, JobStatus::Succeeded, "{:?}", job.error);
        assert!(!job.reseeded);
        let argv = &h.logged("argv")[0];
        let lines: Vec<&str> = argv.lines().collect();
        let r = lines.iter().position(|l| *l == "-r").expect("resumed");
        assert_eq!(lines[r + 1], "prior-session");
        let m = lines.iter().position(|l| *l == "--model").expect("model");
        assert_eq!(lines[m + 1], "claude-haiku-4-5");
        assert!(!h.logged("prompt")[0].contains("SESSION-SEED"));
        let session = h.store.read_session(&draft).unwrap().unwrap();
        assert_eq!(session.turns, 4);
        assert_eq!(session.session_id, FIXTURE_SESSION);
    }

    // TS-51: at the turn cap the session is replaced by a re-seeded one.
    #[tokio::test]
    async fn a_session_at_the_turn_cap_is_reseeded() {
        let h = harness("ok", |c| c.turn_cap = 40);
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        h.inbox
            .add_comment(
                &draft,
                EventAuthor::operator(),
                crate::domain::inbox_events::Thread::Overall,
                "operator context worth replaying",
            )
            .unwrap();
        let worn = AgentSession {
            agent: "claude".into(),
            model: None,
            session_id: "worn-session".into(),
            turns: 40,
        };
        h.store.write_session(&draft, &worn).unwrap();

        let job = h.finish(&h.triage(&draft, "R1")).await;
        assert_eq!(job.status, JobStatus::Succeeded, "{:?}", job.error);
        assert!(job.reseeded);
        assert!(!argv_has(&h.logged("argv")[0], "-r"));
        let prompt = &h.logged("prompt")[0];
        assert!(prompt.contains("SESSION-SEED"));
        assert!(prompt.contains("operator context worth replaying"));
        let session = h.store.read_session(&draft).unwrap().unwrap();
        assert_eq!(session.turns, 1);
        assert_eq!(session.session_id, FIXTURE_SESSION);
    }

    // TS-16: draft-help and convert fixtures parse into their typed outputs.
    #[tokio::test]
    async fn draft_help_and_convert_jobs_yield_typed_results() {
        let h = harness("ok", |_| {});
        let draft = h.item("Importer");
        let help = h
            .svc
            .enqueue(&draft, JobInput::DraftHelp { instruction: None })
            .unwrap();
        let help = h.finish(&help).await;
        assert!(
            matches!(&help.output, Some(JobOutput::DraftHelp(d)) if d.proposed_body.contains("Ship the importer")),
            "{help:?}"
        );
        let convert = h
            .svc
            .enqueue(
                &draft,
                JobInput::Convert {
                    targets: vec!["acme-demo".into()],
                    operator_prompt: None,
                },
            )
            .unwrap();
        let convert = h.finish(&convert).await;
        assert!(
            matches!(&convert.output, Some(JobOutput::Convert(c)) if c.targets[0].project == "acme-demo"),
            "{convert:?}"
        );
        // Neither touched the draft body.
        assert_eq!(h.inbox.get(&draft).is_ok(), true);
    }

    // TS-17: two requests on one item run one after the other, and the second
    // never meets the stream manager's active-run rejection.
    #[tokio::test]
    async fn two_requests_on_one_item_run_serially() {
        let h = harness("slow", |_| {});
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        h.request(&draft, "R2");
        let a = h.triage(&draft, "R1");
        let b = h.triage(&draft, "R2");
        let (a, b) = (h.finish(&a).await, h.finish(&b).await);
        assert_eq!(a.status, JobStatus::Succeeded, "{:?}", a.error);
        assert_eq!(b.status, JobStatus::Succeeded, "{:?}", b.error);
        assert_eq!(h.spawns(), 2);
        assert_eq!(h.peak(), 1);
        assert!(a.finished_at <= b.started_at, "{a:?} {b:?}");
    }

    // TS-18: ten jobs racing onto one item from ten threads; one child at a time.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn ten_racing_jobs_on_one_item_spawn_one_child_at_a_time() {
        let h = harness("slow", |c| c.max_concurrent = 4);
        std::fs::write(h.stub_dir.join("stub.delay"), "0.1").unwrap();
        let draft = h.item("Importer");
        for i in 0..10 {
            h.request(&draft, &format!("R{i}"));
        }
        let ids: Vec<String> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..10)
                .map(|i| {
                    let svc = h.svc.clone();
                    let draft = draft.clone();
                    s.spawn(move || {
                        svc.enqueue(
                            &draft,
                            JobInput::Triage {
                                request_id: format!("R{i}"),
                                attempt: 1,
                            },
                        )
                        .unwrap()
                    })
                })
                .collect();
            handles.into_iter().map(|t| t.join().unwrap()).collect()
        });
        for id in &ids {
            let job = h.finish(id).await;
            assert_eq!(job.status, JobStatus::Succeeded, "{:?}", job.error);
        }
        assert_eq!(h.spawns(), 10);
        assert_eq!(h.peak(), 1);
    }

    // TS-19: with maxConcurrent 2, five items never run more than two children.
    #[tokio::test]
    async fn the_global_cap_bounds_children_across_items() {
        let h = harness("slow", |c| c.max_concurrent = 2);
        std::fs::write(h.stub_dir.join("stub.delay"), "0.4").unwrap();
        let mut ids = vec![];
        for i in 0..5 {
            let draft = h.item(&format!("Item {i}"));
            h.request(&draft, "R");
            ids.push(h.triage(&draft, "R"));
        }
        for id in &ids {
            assert_eq!(h.finish(id).await.status, JobStatus::Succeeded);
        }
        assert_eq!(h.spawns(), 5);
        assert_eq!(h.peak(), 2);
    }

    // TS-67: a draft-help submitted behind a queued triage runs before it.
    #[tokio::test]
    async fn interactive_jobs_jump_queued_triage() {
        let h = harness("slow", |_| {});
        std::fs::write(h.stub_dir.join("stub.delay"), "0.4").unwrap();
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        h.request(&draft, "R2");
        let first = h.triage(&draft, "R1");
        for _ in 0..100 {
            if h.svc.job(&first).unwrap().status == JobStatus::Running {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let queued = h.triage(&draft, "R2");
        let help = h
            .svc
            .enqueue(&draft, JobInput::DraftHelp { instruction: None })
            .unwrap();
        assert_eq!(h.svc.job(&queued).unwrap().status, JobStatus::Queued);
        for id in [&first, &queued, &help] {
            h.finish(id).await;
        }
        assert_eq!(h.order(), ["triage", "draft_help", "triage"]);
    }

    // TS-21 / TS-65: a hung child is killed with its process group at the
    // timeout; the job fails, the sink hears it, and no session is written.
    #[tokio::test]
    async fn a_hung_agent_times_out_and_its_process_group_is_killed() {
        let h = harness("hang", |c| c.timeout_secs = 1);
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        let started = std::time::Instant::now();
        let job = h.finish(&h.triage(&draft, "R1")).await;
        assert_eq!(job.status, JobStatus::Failed);
        assert!(
            job.error.as_deref().unwrap_or("").contains("timed out"),
            "{job:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(h.store.read_session(&draft).unwrap().is_none());
        assert_eq!(h.sink.0.lock().unwrap().len(), 1);

        let pid = h
            .logged("grandchild")
            .pop()
            .expect("grandchild pid")
            .trim()
            .to_string();
        let mut gone = false;
        for _ in 0..50 {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(_) => gone = true,
                Ok(stat) if stat.contains(") Z") => gone = true,
                Ok(_) => {}
            }
            if gone {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(gone, "grandchild {pid} survived");
    }

    // TS-20 (runtime half): a non-zero exit fails the job and the queue moves on.
    #[tokio::test]
    async fn a_failing_agent_fails_the_job_and_the_queue_continues() {
        let h = harness("exit", |_| {});
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        h.request(&draft, "R2");
        let a = h.triage(&draft, "R1");
        let b = h.triage(&draft, "R2");
        let a = h.finish(&a).await;
        assert_eq!(a.status, JobStatus::Failed);
        assert!(a.error.as_deref().unwrap_or("").contains("exit"), "{a:?}");
        assert_eq!(h.finish(&b).await.status, JobStatus::Failed);
        assert_eq!(h.spawns(), 2);
    }

    // TS-22: bad JSON, an unknown field and a wrong version each fail the job
    // without touching the session.
    #[tokio::test]
    async fn invalid_agent_output_fails_the_job_without_mutation() {
        for fixture in [
            "triage_bad_json",
            "triage_unknown_field",
            "triage_wrong_version",
        ] {
            let h = harness(&format!("fixture:{fixture}"), |_| {});
            let draft = h.item("Importer");
            h.request(&draft, "R1");
            let prior = AgentSession {
                agent: "claude".into(),
                model: None,
                session_id: "prior".into(),
                turns: 2,
            };
            h.store.write_session(&draft, &prior).unwrap();
            let job = h.finish(&h.triage(&draft, "R1")).await;
            assert_eq!(job.status, JobStatus::Failed, "{fixture}");
            assert!(job.output.is_none());
            let err = job.error.unwrap_or_default();
            assert!(err.contains("agent output"), "{fixture}: {err}");
            assert_eq!(
                h.store.read_session(&draft).unwrap(),
                Some(prior),
                "{fixture}"
            );
            assert_eq!(h.inbox.list_requests(&draft).unwrap().len(), 1);
        }
    }

    // TS-35 / TS-60: the child starts in an empty per-item scratch dir, with
    // no control token or unlisted variable, read-only and never yolo.
    #[tokio::test]
    async fn the_child_is_isolated() {
        let h = harness("ok", |_| {});
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        let job = h.finish(&h.triage(&draft, "R1")).await;
        assert_eq!(job.status, JobStatus::Succeeded, "{:?}", job.error);

        let cwd = h.logged("cwd").pop().unwrap();
        let expected = std::fs::canonicalize(h.scratch.join(&draft)).unwrap();
        assert_eq!(PathBuf::from(cwd.trim()), expected);
        assert_eq!(
            h.logged("cwdlist").pop().unwrap().trim(),
            "",
            "cwd not empty"
        );

        let env = h.logged("env").pop().unwrap();
        assert!(!env.contains("SEBENZA_CONTROL_TOKEN"), "{env}");
        assert!(!env.contains("must-not-leak"), "{env}");
        assert!(!env.contains("GITHUB_TOKEN"), "{env}");
        for key in env.lines().filter_map(|l| l.split('=').next()) {
            assert!(
                isolation::ENV_ALLOWLIST.contains(&key)
                    || ["PWD", "SHLVL", "_", "OLDPWD"].contains(&key),
                "unexpected variable {key}"
            );
        }
        let argv = h.logged("argv").pop().unwrap();
        assert!(argv_has(&argv, "plan"));
        assert!(!argv.contains("bypassPermissions"));
        assert!(argv_has(&argv, "--disallowedTools"));
    }

    // TS-65: the kill switch spawns nothing, from enqueue or from a new request.
    #[tokio::test]
    async fn the_kill_switch_spawns_nothing() {
        let h = harness("ok", |c| c.enabled = false);
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        assert_eq!(
            h.svc
                .enqueue(&draft, JobInput::DraftHelp { instruction: None }),
            Err(EnqueueError::Disabled)
        );
        h.svc.observer().request_opened(&draft, "R1");
        assert_eq!(h.svc.recover_on_startup(), 0);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(h.svc.jobs_for(&draft).is_empty());
        assert_eq!(h.spawns(), 0);
        assert!(!h.log().exists());
    }

    // A new request reaching InboxService enqueues a triage (FR-23 wiring).
    #[tokio::test]
    async fn a_new_request_enqueues_a_triage_job() {
        use crate::services::inbox_convert::{ConversionOutcome, ConversionTarget};
        let h = harness("ok", |_| {});
        h.inbox.set_request_observer(h.svc.observer());
        let draft = h.item("Importer");
        let target = ConversionTarget {
            project_path: "/repos/acme-demo".into(),
            branch: "feat/importer".into(),
            base_branch: None,
            agent_id: Some("claude".into()),
            prompt: "go".into(),
        };
        let outcome = serde_yaml::to_value(ConversionOutcome::created(
            &target,
            "/worktrees/acme-demo/feat-importer".into(),
            "2026-09-30T00:00:00Z".into(),
        ))
        .unwrap();
        h.store
            .merge_frontmatter(
                &draft,
                crate::adapters::inbox_store::FrontmatterAuthor::Job,
                crate::adapters::inbox_store::FrontmatterPatch {
                    conversions: Some(vec![outcome]),
                    ..Default::default()
                },
            )
            .unwrap();
        let worktree = h.inbox.list_comments(&draft).unwrap().worktrees[0].clone();
        let opened = h
            .inbox
            .open_request(
                &draft,
                EventAuthor::worktree_agent(),
                WorktreeKey {
                    project: worktree.project,
                    branch: worktree.branch,
                },
                "Need help",
                "Where is the loader?",
            )
            .unwrap();
        let InboxEventKind::RequestOpened { request_id, .. } = opened.kind else {
            panic!("request");
        };
        let jobs = h.svc.jobs_for(&draft);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].kind, JobKind::Triage);
        assert_eq!(jobs[0].request_id.as_deref(), Some(request_id.as_str()));
        assert_eq!(jobs[0].attempt, 1);
        let done = h.finish(&jobs[0].job_id).await;
        assert_eq!(done.status, JobStatus::Succeeded, "{:?}", done.error);
    }

    // TD-2: the same (request, kind, attempt) is one job, not two.
    #[tokio::test]
    async fn a_repeated_triage_is_deduped() {
        let h = harness("ok", |_| {});
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        let a = h.triage(&draft, "R1");
        let b = h.triage(&draft, "R1");
        assert_eq!(a, b);
        let retry = h
            .svc
            .enqueue(
                &draft,
                JobInput::Triage {
                    request_id: "R1".into(),
                    attempt: 2,
                },
            )
            .unwrap();
        assert_ne!(retry, a);
        h.finish(&a).await;
        h.finish(&retry).await;
        assert_eq!(h.svc.jobs_for(&draft).len(), 2);
    }

    #[tokio::test]
    async fn a_job_for_an_unknown_draft_is_refused_and_jobs_report_metadata() {
        let h = harness("ok", |_| {});
        assert!(matches!(
            h.svc
                .enqueue("not-a-ulid", JobInput::DraftHelp { instruction: None }),
            Err(EnqueueError::Invalid(_))
        ));
        assert!(h.svc.job("nope").is_none());
        let draft = h.item("Importer");
        let id = h
            .svc
            .enqueue(&draft, JobInput::DraftHelp { instruction: None })
            .unwrap();
        let record = h.finish(&id).await;
        let json = serde_json::to_value(&record).unwrap();
        assert_eq!(json["kind"], "draft_help");
        assert_eq!(json["status"], "succeeded");
        assert_eq!(json["draftId"], draft.as_str());
        assert_eq!(json["output"]["jobKind"], "draft_help");
        assert!(json.get("input").is_none());
    }

    // TS-50: restart re-enqueues each never-triaged open request once.
    #[tokio::test]
    async fn restart_recovery_requeues_untriaged_requests_once() {
        let h = harness("ok", |_| {});
        let a = h.item("A");
        h.request(&a, "R-open");
        h.request(&a, "R-proposed");
        h.store
            .append_event(
                &a,
                EventAuthor::system_agent(),
                None,
                InboxEventKind::Proposal {
                    request_id: "R-proposed".into(),
                    proposal_id: "P1".into(),
                    body: "b".into(),
                    rationale: "r".into(),
                    warnings: vec![],
                },
            )
            .unwrap();
        h.request(&a, "R-flagged");
        h.store
            .append_event(
                &a,
                EventAuthor::system_agent(),
                None,
                InboxEventKind::TriageFailed {
                    request_id: "R-flagged".into(),
                    error: "timed out".into(),
                },
            )
            .unwrap();
        let b = h.item("B");
        h.request(&b, "R-other");
        let dropped = h.item("C");
        h.request(&dropped, "R-dropped");
        h.inbox.drop_draft(&dropped).unwrap();
        assert_eq!(
            h.inbox.get(&dropped).map(|(v, _)| matches!(
                v,
                crate::domain::model::InboxDraftView::Parsed(d) if d.frontmatter.status == DraftStatus::Dropped
            )).unwrap(),
            true
        );

        assert_eq!(h.svc.recover_on_startup(), 2);
        assert_eq!(
            h.svc.recover_on_startup(),
            0,
            "deduped on (request, kind, attempt)"
        );

        let a_jobs = h.svc.jobs_for(&a);
        assert_eq!(a_jobs.len(), 1);
        assert_eq!(a_jobs[0].request_id.as_deref(), Some("R-open"));
        assert_eq!(a_jobs[0].attempt, 1);
        assert_eq!(h.svc.jobs_for(&b).len(), 1);
        assert!(h.svc.jobs_for(&dropped).is_empty());
        for job in a_jobs.iter().chain(h.svc.jobs_for(&b).iter()) {
            h.finish(&job.job_id).await;
        }
    }

    // --- Triage application (phase 4) ---------------------------------------

    /// A harness whose finished jobs are applied to the inbox, as in production.
    fn applied(mode: &str) -> Harness {
        let h = harness(mode, |_| {});
        h.svc
            .set_sink(Arc::new(apply::TriageApplier::new(h.inbox.clone())));
        h
    }

    fn view(h: &Harness, draft: &str, rid: &str) -> crate::domain::inbox_events::RequestView {
        h.inbox.request(draft, rid).expect("request")
    }

    fn priority_of(h: &Harness, draft: &str) -> (Priority, crate::domain::model::PrioritySource) {
        match h.inbox.get(draft).expect("get").0 {
            InboxDraftView::Parsed(d) => (d.frontmatter.priority, d.frontmatter.priority_source),
            _ => panic!("unparsed"),
        }
    }

    fn event_types(h: &Harness, draft: &str) -> Vec<String> {
        h.store
            .read_events(draft)
            .unwrap()
            .iter()
            .map(|e| {
                serde_json::to_value(e).unwrap()["type"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    }

    // TS-14: the stub proposes; priority is set, the proposal is recorded
    // with a server-issued id, and the request is proposed.
    #[tokio::test]
    async fn a_triage_proposal_sets_priority_and_proposes() {
        let h = applied("ok");
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        let done = h.finish(&h.triage(&draft, "R1")).await;
        assert_eq!(done.status, JobStatus::Succeeded, "{:?}", done.error);
        let r = view(&h, &draft, "R1");
        assert_eq!(r.status, RequestStatus::Proposed);
        assert_eq!(
            r.proposal.as_deref(),
            Some("Use the existing fixture loader in tests/helpers.")
        );
        let pid = r.proposal_id.expect("proposal id");
        assert!(!pid.is_empty() && pid != "R1");
        assert_eq!(
            priority_of(&h, &draft),
            (Priority::P1, crate::domain::model::PrioritySource::Agent)
        );
        assert_eq!(
            event_types(&h, &draft),
            ["request_opened", "priority_changed", "proposal"]
        );
    }

    // TS-23: advice is a comment and nothing else; the request stays open.
    #[tokio::test]
    async fn triage_advice_is_only_a_comment() {
        let h = applied("fixture:triage_advice");
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        let done = h.finish(&h.triage(&draft, "R1")).await;
        assert_eq!(done.status, JobStatus::Succeeded, "{:?}", done.error);
        let r = view(&h, &draft, "R1");
        assert_eq!(r.status, RequestStatus::Open);
        assert!(r.proposal.is_none());
        assert!(!r.flagged);
        assert_eq!(priority_of(&h, &draft).0, Priority::P0);
        assert_eq!(
            event_types(&h, &draft),
            ["request_opened", "priority_changed", "advice"]
        );
        let groups = h.inbox.list_comments(&draft).unwrap();
        let advice = groups
            .worktrees
            .iter()
            .flat_map(|g| g.comments.iter())
            .find(|c| c.kind == crate::services::inbox_service::CommentKind::Advice)
            .expect("advice row in the worktree thread");
        assert_eq!(advice.body, "Ask the data team which loader they maintain.");
    }

    // TS-23: no recommendation — priority only.
    #[tokio::test]
    async fn triage_with_no_recommendation_sets_priority_only() {
        let h = applied("fixture:triage_none");
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        let done = h.finish(&h.triage(&draft, "R1")).await;
        assert_eq!(done.status, JobStatus::Succeeded, "{:?}", done.error);
        assert_eq!(view(&h, &draft, "R1").status, RequestStatus::Open);
        assert_eq!(priority_of(&h, &draft).0, Priority::P3);
        assert_eq!(
            event_types(&h, &draft),
            ["request_opened", "priority_changed"]
        );
    }

    // TS-24: an operator override survives triage.
    #[tokio::test]
    async fn triage_never_overrides_the_operator() {
        let h = applied("ok");
        let draft = h.item("Importer");
        h.inbox
            .set_priority(&draft, Some(Priority::P3), None)
            .unwrap();
        h.request(&draft, "R1");
        let done = h.finish(&h.triage(&draft, "R1")).await;
        assert_eq!(done.status, JobStatus::Succeeded, "{:?}", done.error);
        assert_eq!(
            priority_of(&h, &draft),
            (Priority::P3, crate::domain::model::PrioritySource::Operator)
        );
        assert_eq!(view(&h, &draft, "R1").status, RequestStatus::Proposed);
    }

    // TS-20: a failing agent flags the request open; the queue continues.
    #[tokio::test]
    async fn a_failed_triage_flags_the_request_and_the_queue_continues() {
        let h = applied("exit");
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        h.request(&draft, "R2");
        let first = h.triage(&draft, "R1");
        let done = h.finish(&first).await;
        assert_eq!(done.status, JobStatus::Failed);
        let r = view(&h, &draft, "R1");
        assert_eq!(r.status, RequestStatus::Open);
        assert!(r.flagged);
        assert!(r.last_error.as_deref().unwrap_or("").contains("status 3"));
        std::fs::write(h.stub_dir.join("stub.mode"), "ok").unwrap();
        let next = h.finish(&h.triage(&draft, "R2")).await;
        assert_eq!(next.status, JobStatus::Succeeded, "{:?}", next.error);
        assert_eq!(view(&h, &draft, "R2").status, RequestStatus::Proposed);
    }

    // TS-22 applied: unparseable output fails the job and flags the request
    // without a proposal.
    #[tokio::test]
    async fn bad_agent_output_flags_without_a_proposal() {
        let h = applied("fixture:triage_bad_json");
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        h.finish(&h.triage(&draft, "R1")).await;
        let r = view(&h, &draft, "R1");
        assert!(r.flagged);
        assert!(r.proposal.is_none());
        assert_eq!(event_types(&h, &draft), ["request_opened", "triage_failed"]);
    }

    // TS-63: a flagged request is re-triaged as attempt 2 and proposed.
    #[tokio::test]
    async fn retry_triage_reruns_a_flagged_request() {
        let h = applied("exit");
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        h.finish(&h.triage(&draft, "R1")).await;
        assert!(view(&h, &draft, "R1").flagged);
        std::fs::write(h.stub_dir.join("stub.mode"), "ok").unwrap();
        let retry = h.svc.retry_triage(&draft, "R1").expect("retry");
        let done = h.finish(&retry).await;
        assert_eq!(done.attempt, 2);
        assert_eq!(done.status, JobStatus::Succeeded, "{:?}", done.error);
        let r = view(&h, &draft, "R1");
        assert_eq!(r.status, RequestStatus::Proposed);
        assert!(!r.flagged);
        assert!(matches!(
            h.svc.retry_triage(&draft, "R1"),
            Err(EnqueueError::NotRetryable(_))
        ));
        assert!(matches!(
            h.svc.retry_triage(&draft, "ghost"),
            Err(EnqueueError::UnknownRequest(_))
        ));
    }

    // TS-63: a retry after a restart (no job memory) still takes a fresh
    // attempt, from the log's triage_failed count.
    #[tokio::test]
    async fn retry_attempts_survive_a_restart() {
        let h = applied("ok");
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        h.inbox
            .record_triage_failed(&draft, "R1", "timed out")
            .unwrap();
        let retry = h.svc.retry_triage(&draft, "R1").expect("retry");
        assert_eq!(h.finish(&retry).await.attempt, 2);
    }

    #[tokio::test]
    async fn retry_triage_respects_the_kill_switch() {
        let h = harness("ok", |c| c.enabled = false);
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        assert_eq!(
            h.svc.retry_triage(&draft, "R1"),
            Err(EnqueueError::Disabled)
        );
    }

    // FR-20: subscribers see each job move through its states.
    #[tokio::test]
    async fn job_updates_are_published() {
        let h = applied("ok");
        let draft = h.item("Importer");
        h.request(&draft, "R1");
        let mut rx = h.svc.subscribe();
        let job = h.triage(&draft, "R1");
        let mut seen = Vec::new();
        while let Ok(Ok(update)) = tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
            assert_eq!(update.job_id, job);
            seen.push(update.status);
            if update.status.is_terminal() {
                break;
            }
        }
        assert_eq!(
            seen,
            [JobStatus::Queued, JobStatus::Running, JobStatus::Succeeded]
        );
    }

    // TS-36: item B's triage prompt shows item A's title and priority only.
    #[tokio::test]
    async fn another_items_body_and_comments_never_reach_the_prompt() {
        use crate::adapters::inbox_store::EventAuthor;
        use crate::domain::inbox_events::Thread;
        let h = applied("ok");
        let a = h.item("Item A rollout");
        let hash = match h.inbox.get(&a).unwrap().0 {
            InboxDraftView::Parsed(d) => {
                crate::domain::model::FileRevision::of_body(&d.body).body_hash
            }
            _ => panic!(),
        };
        h.inbox.save_body(&a, &hash, "A-PRIVATE-BODY").unwrap();
        h.inbox
            .add_comment(
                &a,
                EventAuthor::operator(),
                Thread::Overall,
                "A-PRIVATE-COMMENT",
            )
            .unwrap();
        let b = h.item("Item B");
        h.request(&b, "R1");
        h.finish(&h.triage(&b, "R1")).await;
        let prompt = h.logged("prompt").pop().expect("prompt");
        assert!(prompt.contains("Item A rollout"), "digest lists A's title");
        assert!(!prompt.contains("A-PRIVATE-BODY"), "{prompt}");
        assert!(!prompt.contains("A-PRIVATE-COMMENT"), "{prompt}");
    }
}
