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

use super::{JobKind, JobRecord, JobResultSink, LoggingJobSink};
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
}

impl JobResultSink for TriageApplier {
    fn job_finished(&self, job: &JobRecord) {
        LoggingJobSink.job_finished(job);
        if job.kind != JobKind::Triage {
            return;
        }
        let _ = &self.inbox;
        todo!("phase-4-task-2")
    }
}
