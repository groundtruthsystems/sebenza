//! Applying finished triage jobs to the inbox (AA-D2): the agent only
//! answers; this sink makes every mutation, through [`InboxService`].
//!
//! - priority: [`InboxService::set_agent_priority`], a no-op while an
//!   operator override stands (BR-02);
//! - `proposal`: a server-issued `proposal` event, so the request becomes
//!   proposed and waits for the operator's confirm;
//! - `advice`: an `advice` comment in the request's worktree thread, never
//!   delivered (BR-07);
//! - `none`: priority only (UC-05b);
//! - a failed, timed-out or unparseable job: `triage_failed`, leaving the
//!   request open and flagged for a retry (UC-05a).
//!
//! Nothing here can deliver: delivery is reachable only from the operator's
//! confirm and redeliver (T-02).

use super::{JobKind, JobOutput, JobRecord, JobResultSink, JobStatus, LoggingJobSink};
use super::{Recommendation, TriageOutput};
use crate::services::inbox_service::InboxService;
use std::sync::Arc;

/// The production [`JobResultSink`].
pub struct TriageApplier {
    inbox: Arc<InboxService>,
}

impl TriageApplier {
    pub fn new(inbox: Arc<InboxService>) -> Self {
        Self { inbox }
    }

    /// Apply a valid triage result. An error here (the request vanished, the
    /// proposal is over the body cap) is recorded as a failed triage.
    fn apply(&self, draft_id: &str, request_id: &str, out: &TriageOutput) -> Result<(), String> {
        self.inbox
            .set_agent_priority(draft_id, out.priority)
            .map_err(|e| format!("cannot set priority: {e}"))?;
        let body = out.body.as_deref().unwrap_or("");
        match out.recommendation {
            Recommendation::None => {}
            Recommendation::Advice => {
                self.inbox
                    .record_advice(draft_id, request_id, body)
                    .map_err(|e| format!("cannot record advice: {e}"))?;
            }
            Recommendation::Proposal => {
                let recorded = self
                    .inbox
                    .record_proposal(draft_id, request_id, body, &out.rationale)
                    .map_err(|e| format!("cannot record the proposal: {e}"))?;
                if recorded.is_none() {
                    // The operator decided while triage ran; theirs stands.
                    tracing::info!(
                        draft_id,
                        request_id,
                        "triage proposal dropped: the request is no longer open"
                    );
                }
            }
        }
        Ok(())
    }
}

impl JobResultSink for TriageApplier {
    fn job_finished(&self, job: &JobRecord) {
        LoggingJobSink.job_finished(job);
        if job.kind != JobKind::Triage {
            return;
        }
        let Some(request_id) = job.request_id.as_deref() else {
            return;
        };
        let outcome = match (job.status, &job.output) {
            (JobStatus::Succeeded, Some(JobOutput::Triage(out))) => {
                self.apply(&job.draft_id, request_id, out)
            }
            (JobStatus::Succeeded, _) => Err("triage produced no triage result".to_string()),
            _ => Err(job
                .error
                .clone()
                .unwrap_or_else(|| "the triage job failed".to_string())),
        };
        if let Err(error) = outcome
            && let Err(e) = self
                .inbox
                .record_triage_failed(&job.draft_id, request_id, &error)
        {
            tracing::warn!(
                draft_id = %job.draft_id,
                request_id,
                "cannot flag the failed triage: {e}"
            );
        }
    }
}
