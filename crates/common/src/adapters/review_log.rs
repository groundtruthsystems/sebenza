//! The per-project review log: `~/.ai/sebenza/reviews/<project>/events.jsonl`.
//!
//! Append-only and fsynced, like the inbox `events.jsonl`: one `write` of the
//! whole line, then a data sync, which is the commit point. Review state is a
//! fold over the log. A crash mid-write leaves a torn final line; it never
//! held a committed event, so it is cut off before the next read or append.
//! Events carry ids and hashes only, never code, tokens or free text.

use crate::adapters::inbox_store::write_durably;
use crate::domain::review_records::{
    AttemptFailed, ExistingChecksRecorded, FindingDecided, JobBlocked, PlanApproved, RunLeased,
    RunSubmitted,
};
use crate::util::id::random_ulid;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The event schema this binary writes and reads.
pub const REVIEW_LOG_SCHEMA_VERSION: u32 = 1;

/// How a completion reached the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionSource {
    /// `POST /tracks/{id}/complete`.
    Daemon,
    /// The reconciler found the track marked done with no completion event.
    Reconciled,
}

/// What a review job is unique on. A repeat completion with the same key is
/// a no-op.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JobKey {
    pub track_id: String,
    pub head_sha: String,
    pub base_sha: String,
    pub standards_rev: String,
}

/// A track's completion and the review job it enqueues, in one event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackCompleted {
    pub track_id: String,
    pub head_sha: String,
    pub base_sha: String,
    pub standards_rev: String,
    pub source: CompletionSource,
    /// The credential class that asked (`control`), `None` for the reconciler.
    pub credential: Option<String>,
    /// Where the `tracks.json` projection of this completion lives.
    pub worktree_path: String,
    pub branch: String,
}

impl JobKey {
    /// A stable id for the job: the first 24 hex digits of the SHA-256 of
    /// its key fields.
    pub fn job_id(&self) -> String {
        let joined = [
            self.track_id.as_str(),
            &self.head_sha,
            &self.base_sha,
            &self.standards_rev,
        ]
        .join("\0");
        let mut id = crate::domain::solution::sha256_hex(&joined);
        id.truncate(24);
        format!("job_{id}")
    }
}

impl TrackCompleted {
    /// The job key this completion enqueues.
    pub fn job_key(&self) -> JobKey {
        JobKey {
            track_id: self.track_id.clone(),
            head_sha: self.head_sha.clone(),
            base_sha: self.base_sha.clone(),
            standards_rev: self.standards_rev.clone(),
        }
    }
}

/// Every kind of review-log event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ReviewEventKind {
    TrackCompleted(TrackCompleted),
    RunLeased(RunLeased),
    AttemptFailed(AttemptFailed),
    JobBlocked(JobBlocked),
    RunSubmitted(RunSubmitted),
    ExistingChecksRecorded(ExistingChecksRecorded),
    PlanApproved(PlanApproved),
    FindingDecided(FindingDecided),
}

/// One line of the review log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub ts: String,
    #[serde(flatten)]
    pub kind: ReviewEventKind,
}

impl ReviewEvent {
    /// A new event stamped now.
    pub fn new(kind: ReviewEventKind) -> Self {
        Self {
            schema_version: REVIEW_LOG_SCHEMA_VERSION,
            event_id: random_ulid(),
            ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            kind,
        }
    }
}

/// The fold of a review log.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewState {
    /// Every completion, in log order. Each is one review job.
    pub completions: Vec<TrackCompleted>,
}

impl ReviewState {
    /// Fold events in log order.
    pub fn fold<'a>(events: impl IntoIterator<Item = &'a ReviewEvent>) -> Self {
        let mut state = Self::default();
        for event in events {
            if let ReviewEventKind::TrackCompleted(c) = &event.kind {
                state.completions.push(c.clone());
            }
        }
        state
    }

    /// The latest completion of `track_id`.
    pub fn completion_for(&self, track_id: &str) -> Option<&TrackCompleted> {
        self.completions
            .iter()
            .rev()
            .find(|c| c.track_id == track_id)
    }

    /// The completion that enqueued the job `key`, if any.
    pub fn job(&self, key: &JobKey) -> Option<&TrackCompleted> {
        self.completions.iter().find(|c| &c.job_key() == key)
    }

    /// How many jobs `track_id` has.
    pub fn job_count(&self, track_id: &str) -> usize {
        self.completions
            .iter()
            .filter(|c| c.track_id == track_id)
            .count()
    }

    /// The latest completion of each track, in first-completed order.
    pub fn latest_completions(&self) -> Vec<&TrackCompleted> {
        let mut seen = std::collections::HashSet::new();
        let mut latest: Vec<&TrackCompleted> = self
            .completions
            .iter()
            .rev()
            .filter(|c| seen.insert(c.track_id.as_str()))
            .collect();
        latest.reverse();
        latest
    }
}

/// One project's review log.
#[derive(Debug, Clone)]
pub struct ReviewLog {
    path: PathBuf,
}

/// The review-log directory for every project: `~/.ai/sebenza/reviews`.
pub fn default_review_root() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home)
        .join(".ai")
        .join("sebenza")
        .join("reviews")
}

impl ReviewLog {
    /// The log at `path`. Nothing is created until the first append.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// The log file of `project_key` under `root`.
    pub fn for_project(root: &Path, project_key: &str) -> Self {
        Self::new(root.join(project_key).join("events.jsonl"))
    }

    /// The file this log reads and appends.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Open the log: cut a torn final line, then return the fold.
    pub fn open(&self) -> std::io::Result<ReviewState> {
        let Some(mut file) = self.locked_file(false)? else {
            return Ok(ReviewState::default());
        };
        let events = read_repaired(&mut file)?;
        Ok(ReviewState::fold(&events))
    }

    /// Append `completion` durably unless `existing` finds a completion that
    /// already covers it in the current fold. The check and the append share
    /// one lock. Returns the covering completion and whether it was appended.
    pub fn append_completion_unless(
        &self,
        completion: TrackCompleted,
        existing: impl FnOnce(&ReviewState) -> Option<TrackCompleted>,
    ) -> std::io::Result<(TrackCompleted, bool)> {
        let mut file = self
            .locked_file(true)?
            .ok_or_else(|| std::io::Error::other("review log missing after create"))?;
        let events = read_repaired(&mut file)?;
        if let Some(found) = existing(&ReviewState::fold(&events)) {
            return Ok((found, false));
        }
        append_event(
            &mut file,
            ReviewEventKind::TrackCompleted(completion.clone()),
        )?;
        Ok((completion, true))
    }

    /// Every event, in log order, after cutting a torn final line.
    pub fn events(&self) -> std::io::Result<Vec<ReviewEvent>> {
        match self.locked_file(false)? {
            Some(mut file) => read_repaired(&mut file),
            None => Ok(Vec::new()),
        }
    }

    /// Run `decide` over the current events under the log's lock and
    /// durably append the one event it returns, if any. The decision and the
    /// append share the lock, so a check inside `decide` cannot race another
    /// writer. One event per call keeps each commit point a single line.
    pub fn transact<T, E: From<std::io::Error>>(
        &self,
        decide: impl FnOnce(&[ReviewEvent]) -> Result<(Option<ReviewEventKind>, T), E>,
    ) -> Result<T, E> {
        let mut file = self
            .locked_file(true)?
            .ok_or_else(|| std::io::Error::other("review log missing after create"))?;
        let events = read_repaired(&mut file)?;
        let (event, out) = decide(&events)?;
        if let Some(kind) = event {
            append_event(&mut file, kind)?;
        }
        Ok(out)
    }

    /// The log file opened for read and append under an exclusive lock that
    /// lasts until it is dropped. `None` when it does not exist and `create`
    /// is false.
    fn locked_file(&self, create: bool) -> std::io::Result<Option<fs::File>> {
        if create {
            self.ensure_dir()?;
        }
        let existed = self.path.exists();
        let opened = fs::OpenOptions::new()
            .read(true)
            .append(true)
            .create(create)
            .mode(0o600)
            .open(&self.path);
        let file = match opened {
            Ok(f) => f,
            Err(e) if !create && e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        file.lock()?;
        if !existed {
            sync_parent(&self.path)?;
        }
        Ok(Some(file))
    }

    fn ensure_dir(&self) -> std::io::Result<()> {
        let Some(dir) = self.path.parent() else {
            return Ok(());
        };
        fs::create_dir_all(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
    }
}

/// Append `kind` as one stamped line: one write, then a data sync.
fn append_event(file: &mut fs::File, kind: ReviewEventKind) -> std::io::Result<()> {
    let mut line = serde_json::to_string(&ReviewEvent::new(kind))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    line.push('\n');
    write_durably(file, line.as_bytes())
}

/// A new log's directory entry must survive a crash too.
fn sync_parent(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(dir) => fs::File::open(dir)?.sync_all(),
        None => Ok(()),
    }
}

/// Read every event, first truncating the file after its last newline so a
/// torn tail can never merge with the next append.
fn read_repaired(file: &mut fs::File) -> std::io::Result<Vec<ReviewEvent>> {
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(0))?;
    file.read_to_end(&mut bytes)?;
    let keep = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    if keep < bytes.len() {
        file.set_len(keep as u64)?;
        file.sync_data()?;
        bytes.truncate(keep);
    }
    Ok(parse_events(&bytes))
}

/// Every whole, parseable line. Unknown or damaged lines are skipped rather
/// than failing the fold.
fn parse_events(bytes: &[u8]) -> Vec<ReviewEvent> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|l| serde_json::from_str::<ReviewEvent>(l).ok())
        .filter(|e| e.schema_version <= REVIEW_LOG_SCHEMA_VERSION)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_log(name: &str) -> ReviewLog {
        let dir = std::env::temp_dir().join(format!(
            "sebenza-review-log-{}-{name}-{}",
            std::process::id(),
            random_ulid()
        ));
        ReviewLog::for_project(&dir, "proj")
    }

    fn completion(track: &str, head: &str) -> TrackCompleted {
        TrackCompleted {
            track_id: track.into(),
            head_sha: head.into(),
            base_sha: "b".repeat(40),
            standards_rev: "r".repeat(64),
            source: CompletionSource::Daemon,
            credential: Some("control".into()),
            worktree_path: "/wt".into(),
            branch: "feat".into(),
        }
    }

    fn append_once(log: &ReviewLog, c: TrackCompleted) -> bool {
        let key = c.job_key();
        log.append_completion_unless(c, |s| s.job(&key).cloned())
            .unwrap()
            .1
    }

    #[test]
    fn an_append_is_one_fsynced_line_that_the_fold_returns() {
        let log = temp_log("append");
        assert!(append_once(&log, completion("t1", &"a".repeat(40))));
        let text = fs::read_to_string(log.path()).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.ends_with('\n'));
        let event: ReviewEvent = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(event.schema_version, REVIEW_LOG_SCHEMA_VERSION);
        assert!(text.contains("\"kind\":\"track-completed\""));
        let state = log.open().unwrap();
        assert_eq!(state.completions, vec![completion("t1", &"a".repeat(40))]);
    }

    #[test]
    fn the_log_and_its_directory_are_owner_only() {
        let log = temp_log("perms");
        append_once(&log, completion("t1", "h"));
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(log.path()), 0o600);
        assert_eq!(mode(log.path().parent().unwrap()), 0o700);
    }

    #[test]
    fn a_repeat_with_the_same_job_key_appends_nothing() {
        let log = temp_log("idem");
        assert!(append_once(&log, completion("t1", "h1")));
        assert!(!append_once(&log, completion("t1", "h1")));
        assert!(append_once(&log, completion("t1", "h2")));
        let state = log.open().unwrap();
        assert_eq!(state.job_count("t1"), 2);
        assert_eq!(state.completion_for("t1").unwrap().head_sha, "h2");
    }

    #[test]
    fn ts69_opening_discards_a_torn_final_line_and_recovers_the_prior_fold() {
        let log = temp_log("torn");
        append_once(&log, completion("t1", "h1"));
        let before = log.open().unwrap();
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(log.path())
            .unwrap();
        f.write_all(br#"{"schema_version":1,"event_id":"x","kind":"track-comp"#)
            .unwrap();
        drop(f);
        assert_eq!(log.open().unwrap(), before);
        let text = fs::read_to_string(log.path()).unwrap();
        assert!(text.ends_with('\n'), "the torn tail is cut: {text:?}");
        assert_eq!(text.lines().count(), 1);
    }

    #[test]
    fn an_append_after_a_torn_tail_starts_a_fresh_line() {
        let log = temp_log("torn-append");
        append_once(&log, completion("t1", "h1"));
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(log.path())
            .unwrap();
        f.write_all(b"{\"torn").unwrap();
        drop(f);
        assert!(append_once(&log, completion("t2", "h2")));
        let state = log.open().unwrap();
        assert_eq!(state.completions.len(), 2);
    }

    #[test]
    fn opening_a_missing_log_is_an_empty_fold_and_creates_nothing() {
        let log = temp_log("missing");
        assert_eq!(log.open().unwrap(), ReviewState::default());
        assert!(!log.path().exists());
    }

    #[test]
    fn concurrent_appends_of_one_key_store_one_job() {
        let log = temp_log("race");
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let log = log.clone();
                std::thread::spawn(move || append_once(&log, completion("t1", "h1")))
            })
            .collect();
        let appended = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|a| *a)
            .count();
        assert_eq!(appended, 1);
        assert_eq!(log.open().unwrap().job_count("t1"), 1);
    }

    #[test]
    fn latest_completions_keep_one_per_track() {
        let state = ReviewState {
            completions: vec![
                completion("t1", "h1"),
                completion("t2", "h1"),
                completion("t1", "h2"),
            ],
        };
        let latest: Vec<_> = state
            .latest_completions()
            .iter()
            .map(|c| (c.track_id.clone(), c.head_sha.clone()))
            .collect();
        assert_eq!(
            latest,
            vec![("t2".into(), "h1".into()), ("t1".into(), "h2".into())]
        );
    }
    fn leased(run: &str) -> ReviewEventKind {
        ReviewEventKind::RunLeased(RunLeased {
            job_id: "job_1".into(),
            track_id: "t1".into(),
            run_id: run.into(),
            kind: crate::domain::review_records::RunKind::First,
            attempt: 1,
            lease_id: "l1".into(),
            token_hash: "h".repeat(64),
            lease_until: 100,
        })
    }

    #[test]
    fn a_transaction_appends_the_one_event_it_decides_on() {
        let log = temp_log("tx");
        append_once(&log, completion("t1", "h1"));
        let seen = log
            .transact(|events| Ok::<_, std::io::Error>((Some(leased("run_1")), events.len())))
            .unwrap();
        assert_eq!(seen, 1);
        let events = log.events().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].kind, leased("run_1"));
        assert!(
            fs::read_to_string(log.path())
                .unwrap()
                .contains("\"kind\":\"run-leased\"")
        );
        assert_eq!(
            log.open().unwrap().completions.len(),
            1,
            "runs leave completions alone"
        );
    }

    #[test]
    fn a_transaction_that_decides_nothing_or_fails_appends_nothing() {
        let log = temp_log("tx-none");
        append_once(&log, completion("t1", "h1"));
        log.transact(|_| Ok::<_, std::io::Error>((None, ())))
            .unwrap();
        let failed: Result<(), std::io::Error> =
            log.transact(|_| Err(std::io::Error::other("refused")));
        assert!(failed.is_err());
        assert_eq!(log.events().unwrap().len(), 1);
    }

    #[test]
    fn events_of_a_missing_log_are_empty() {
        assert!(temp_log("ev-missing").events().unwrap().is_empty());
    }

    #[test]
    fn a_job_id_is_stable_and_differs_per_key() {
        let a = completion("t1", "h1").job_key();
        let b = completion("t1", "h2").job_key();
        assert_eq!(a.job_id(), a.clone().job_id());
        assert_ne!(a.job_id(), b.job_id());
        assert!(a.job_id().starts_with("job_"));
        assert_eq!(a.job_id().len(), 28);
    }
}
