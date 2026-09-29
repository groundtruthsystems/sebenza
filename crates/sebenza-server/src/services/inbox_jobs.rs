//! Conversion jobs: the background fan-out of one draft into worktrees.
//!
//! Mirrors `agent_stream`'s shape — a `Mutex<HashMap>` of runs, each with a
//! broadcast channel and a replay buffer — but nothing is shared with it.
//! `ws_agents_stream` is project-prefixed and keyed by conversation id, and a
//! conversion is neither: it is cross-project and is not a conversation.
//!
//! Job ids are ULIDs, and an id is the only thing gating access to a job's
//! progress. That progress carries project paths, branch names and prompt text,
//! so a guessable id would be a cross-project disclosure — hence the same
//! opacity commitment `DraftId` gets.

use common::services::inbox_convert::ConversionOutcome;
use common::util::id::random_ulid;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// How many finished jobs to keep. Progress is read shortly after the fan-out,
/// and the durable record lives in the draft's frontmatter, so this is a
/// convenience cache rather than a store.
const MAX_RETAINED_JOBS: usize = 64;

/// One event on a job's channel.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum JobEvent {
    /// One target finished, for better or worse.
    ///
    /// Boxed so the whole enum is not sized by its largest variant: every
    /// broadcast clone would otherwise carry that much.
    Outcome {
        index: usize,
        total: usize,
        outcome: Box<ConversionOutcome>,
    },
    /// The wave is over. `created` counts targets that actually produced a
    /// working worktree.
    Done { created: usize, total: usize },
    /// The wave never started, or died. Targets already reported still stand.
    Failed { error: String },
}

/// A job's state, as a caller sees it.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSnapshot {
    pub id: String,
    pub draft_id: String,
    pub total: usize,
    pub outcomes: Vec<ConversionOutcome>,
    pub finished: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

struct JobState {
    draft_id: String,
    total: usize,
    outcomes: Vec<ConversionOutcome>,
    finished: bool,
    error: Option<String>,
}

struct Job {
    state: Mutex<JobState>,
    tx: broadcast::Sender<JobEvent>,
}

/// What a subscriber gets on connect: everything that already happened, then a
/// live receiver. Without the replay, a client attaching a moment after the
/// first target finished would never learn of it.
pub struct JobSubscription {
    pub snapshot: JobSnapshot,
    pub receiver: broadcast::Receiver<JobEvent>,
}

#[derive(Default)]
pub struct ConversionJobManager {
    jobs: Mutex<HashMap<String, Arc<Job>>>,
    /// Insertion order, for eviction.
    order: Mutex<Vec<String>>,
}

impl ConversionJobManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a job and return its id.
    pub fn start(&self, draft_id: &str, total: usize) -> String {
        let id = random_ulid();
        let (tx, _) = broadcast::channel(256);
        let job = Arc::new(Job {
            state: Mutex::new(JobState {
                draft_id: draft_id.to_string(),
                total,
                outcomes: Vec::new(),
                finished: false,
                error: None,
            }),
            tx,
        });
        self.jobs.lock().unwrap().insert(id.clone(), job);
        let mut order = self.order.lock().unwrap();
        order.push(id.clone());
        while order.len() > MAX_RETAINED_JOBS {
            let oldest = order.remove(0);
            self.jobs.lock().unwrap().remove(&oldest);
        }
        id
    }

    fn job(&self, id: &str) -> Option<Arc<Job>> {
        self.jobs.lock().unwrap().get(id).cloned()
    }

    /// Record one target's outcome and tell every listener.
    pub fn push_outcome(&self, id: &str, outcome: ConversionOutcome) {
        let Some(job) = self.job(id) else { return };
        let (index, total) = {
            let mut state = job.state.lock().unwrap();
            state.outcomes.push(outcome.clone());
            (state.outcomes.len() - 1, state.total)
        };
        // A send with no subscribers is not an error: the CLI polls instead.
        let _ = job.tx.send(JobEvent::Outcome {
            index,
            total,
            outcome: Box::new(outcome),
        });
    }

    /// Close the job normally.
    pub fn finish(&self, id: &str) {
        let Some(job) = self.job(id) else { return };
        let (created, total) = {
            let mut state = job.state.lock().unwrap();
            state.finished = true;
            (
                state.outcomes.iter().filter(|o| o.is_created()).count(),
                state.total,
            )
        };
        let _ = job.tx.send(JobEvent::Done { created, total });
    }

    /// Close the job with an error. Outcomes already recorded are kept — a
    /// wave that died after two targets still produced two worktrees.
    pub fn fail(&self, id: &str, error: String) {
        let Some(job) = self.job(id) else { return };
        {
            let mut state = job.state.lock().unwrap();
            state.finished = true;
            state.error = Some(error.clone());
        }
        let _ = job.tx.send(JobEvent::Failed { error });
    }

    /// Current state, or `None` for an id that does not exist. Callers turn
    /// that into a 404; it is the whole access check.
    pub fn snapshot(&self, id: &str) -> Option<JobSnapshot> {
        let job = self.job(id)?;
        let state = job.state.lock().unwrap();
        Some(JobSnapshot {
            id: id.to_string(),
            draft_id: state.draft_id.clone(),
            total: state.total,
            outcomes: state.outcomes.clone(),
            finished: state.finished,
            error: state.error.clone(),
        })
    }

    /// Subscribe to a job, with a replay of what already happened.
    pub fn subscribe(&self, id: &str) -> Option<JobSubscription> {
        let job = self.job(id)?;
        let receiver = job.tx.subscribe();
        let snapshot = self.snapshot(id)?;
        Some(JobSubscription { snapshot, receiver })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::services::inbox_convert::ConversionTarget;

    fn target(branch: &str) -> ConversionTarget {
        ConversionTarget {
            project_path: "/code/acme".into(),
            branch: branch.into(),
            base_branch: None,
            agent_id: None,
            prompt: "go".into(),
        }
    }

    fn created(branch: &str) -> ConversionOutcome {
        ConversionOutcome::created(&target(branch), format!("/wt/{branch}"), "t".into())
    }

    fn failed(branch: &str) -> ConversionOutcome {
        ConversionOutcome::failed(&target(branch), "boom".into(), "t".into())
    }

    #[test]
    fn a_job_id_is_a_ulid() {
        // Progress carries project paths and prompt text, so the id is the
        // access control; a guessable one would leak across projects.
        let m = ConversionJobManager::new();
        let id = m.start("01DRAFT", 2);
        assert!(common::util::id::is_ulid(&id), "{id}");
        assert_ne!(id, m.start("01DRAFT", 2));
    }

    #[test]
    fn an_unknown_id_yields_nothing() {
        let m = ConversionJobManager::new();
        m.start("01DRAFT", 1);
        assert!(m.snapshot("01ARZ3NDEKTSV4RRFFQ69G5FAV").is_none());
        assert!(m.subscribe("01ARZ3NDEKTSV4RRFFQ69G5FAV").is_none());
        // Pushing to an unknown job must not panic or create one.
        m.push_outcome("01ARZ3NDEKTSV4RRFFQ69G5FAV", created("x"));
        assert!(m.snapshot("01ARZ3NDEKTSV4RRFFQ69G5FAV").is_none());
    }

    #[test]
    fn outcomes_accumulate_in_order() {
        let m = ConversionJobManager::new();
        let id = m.start("01DRAFT", 3);
        m.push_outcome(&id, created("one"));
        m.push_outcome(&id, failed("two"));
        m.push_outcome(&id, created("three"));

        let snap = m.snapshot(&id).expect("snapshot");
        assert_eq!(snap.total, 3);
        assert_eq!(
            snap.outcomes
                .iter()
                .map(|o| o.branch.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "two", "three"]
        );
        assert!(!snap.finished);
    }

    #[test]
    fn finishing_counts_only_created_targets() {
        let m = ConversionJobManager::new();
        let id = m.start("01DRAFT", 3);
        m.push_outcome(&id, created("one"));
        m.push_outcome(&id, failed("two"));
        m.finish(&id);

        let snap = m.snapshot(&id).expect("snapshot");
        assert!(snap.finished);
        assert!(snap.error.is_none());
        assert_eq!(snap.outcomes.len(), 2);
    }

    #[test]
    fn a_failed_job_keeps_the_targets_that_did_finish() {
        // A wave that died after two targets still produced two worktrees;
        // discarding that record would strand them.
        let m = ConversionJobManager::new();
        let id = m.start("01DRAFT", 4);
        m.push_outcome(&id, created("one"));
        m.push_outcome(&id, created("two"));
        m.fail(&id, "server went down".into());

        let snap = m.snapshot(&id).expect("snapshot");
        assert!(snap.finished);
        assert_eq!(snap.error.as_deref(), Some("server went down"));
        assert_eq!(snap.outcomes.len(), 2);
    }

    #[tokio::test]
    async fn a_subscriber_replays_what_it_missed_then_streams() {
        let m = ConversionJobManager::new();
        let id = m.start("01DRAFT", 2);
        // Happens before anyone is listening.
        m.push_outcome(&id, created("one"));

        let sub = m.subscribe(&id).expect("subscribe");
        assert_eq!(
            sub.snapshot.outcomes.len(),
            1,
            "a late subscriber must still learn about target one"
        );

        let mut rx = sub.receiver;
        m.push_outcome(&id, created("two"));
        m.finish(&id);

        match rx.recv().await.expect("outcome event") {
            JobEvent::Outcome {
                index,
                total,
                outcome,
            } => {
                assert_eq!(index, 1);
                assert_eq!(total, 2);
                assert_eq!(outcome.branch, "two");
            }
            other => panic!("expected an outcome, got {other:?}"),
        }
        match rx.recv().await.expect("done event") {
            JobEvent::Done { created, total } => {
                assert_eq!(created, 2);
                assert_eq!(total, 2);
            }
            other => panic!("expected done, got {other:?}"),
        }
    }

    #[test]
    fn pushing_with_no_subscribers_is_fine() {
        // The CLI polls rather than subscribing, so most jobs have no
        // listeners at all; a send error must not derail the fan-out.
        let m = ConversionJobManager::new();
        let id = m.start("01DRAFT", 1);
        m.push_outcome(&id, created("x"));
        m.finish(&id);
        assert!(m.snapshot(&id).expect("snapshot").finished);
    }

    #[test]
    fn old_jobs_are_evicted_so_memory_stays_bounded() {
        let m = ConversionJobManager::new();
        let first = m.start("01DRAFT", 1);
        for _ in 0..MAX_RETAINED_JOBS {
            m.start("01DRAFT", 1);
        }
        assert!(
            m.snapshot(&first).is_none(),
            "the oldest job should have been evicted"
        );
    }
}
