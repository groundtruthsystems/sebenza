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

pub mod isolation;
pub mod output;
pub mod prompt;

pub use output::{
    ConvertOutput, ConvertTargetOutput, DraftHelpOutput, JobOutput, OutputError, Recommendation,
    TriageOutput,
};

use crate::domain::config::SystemAgentConfig;
use crate::services::agent_stream::AgentStreamManager;
use crate::services::inbox_service::{InboxService, RequestObserver};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

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
}

impl SystemAgentService {
    pub fn new(
        config: SystemAgentConfig,
        inbox: Arc<InboxService>,
        stream: Arc<AgentStreamManager>,
        options: SystemAgentOptions,
    ) -> Arc<Self> {
        Arc::new(Self {
            config,
            inbox,
            stream,
            options,
            sink: RwLock::new(Arc::new(LoggingJobSink)),
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
        todo!("phase-3-task-4")
    }

    /// Queue a job and return its id. A triage already known for the same
    /// `(request_id, attempt)` returns the existing job's id instead (TD-2).
    pub fn enqueue(&self, draft_id: &str, input: JobInput) -> Result<String, EnqueueError> {
        todo!("phase-3-task-4: {draft_id} {input:?}")
    }

    /// One job by id.
    pub fn job(&self, job_id: &str) -> Option<JobRecord> {
        todo!("phase-3-task-4: {job_id}")
    }

    /// An item's known jobs, oldest first.
    pub fn jobs_for(&self, draft_id: &str) -> Vec<JobRecord> {
        todo!("phase-3-task-4: {draft_id}")
    }

    /// Wait for a job to finish. `None` for an unknown id.
    pub async fn wait(&self, job_id: &str) -> Option<JobRecord> {
        todo!("phase-3-task-4: {job_id}")
    }

    /// Re-enqueue, once, every open request on a live item that was never
    /// triaged: no proposal, not flagged, not rejected. Returns how many jobs
    /// were queued (TD-2, FR-21).
    pub fn recover_on_startup(&self) -> usize {
        todo!("phase-3-task-6")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::inbox_store::{EventAuthor, InboxStore};
    use crate::adapters::projects_registry::ProjectsRegistry;
    use crate::domain::inbox_events::{AgentSession, InboxEventKind, WorktreeKey};
    use crate::domain::model::{DraftStatus, Priority};
    use std::sync::Mutex;
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
}
