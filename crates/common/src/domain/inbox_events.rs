//! Collaboration events for one inbox item, stored one JSON object per line in
//! `<ulid>.events.jsonl` beside the draft.
//!
//! Events are immutable. Request state is never stored: it is a fold over the
//! log ([`fold_requests`]), so the log is the single source of truth for
//! comments, requests, proposals, decisions and deliveries.

use super::model::{Priority, PrioritySource};
use super::solution::{
    AgentProfile, CredentialClass, NoteAuthor, RequestOrigin, ReviewLevel, ReviewVerdict, RoundRef,
    SolutionPhase,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const INBOX_EVENT_SCHEMA_VERSION: i32 = 1;

/// Which kind of principal wrote an event. `caller` on worktree events is
/// self-declared and unauthenticated (see the design's T-01).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthorKind {
    Operator,
    WorktreeAgent,
    SystemAgent,
}

/// Identifies one worktree created from the item: its project and branch.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorktreeKey {
    pub project: String,
    pub branch: String,
}

/// Where a comment is shown: the item's overall thread or one worktree's group.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Thread {
    Overall,
    Worktree(WorktreeKey),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InboxEventKind {
    Comment {
        thread: Thread,
        body: String,
        /// Secret/PHI scan hit names; the body is stored unchanged (warn only).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<String>,
    },
    RequestOpened {
        request_id: String,
        worktree: WorktreeKey,
        title: String,
        body: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<String>,
        /// The plan task the agent is blocked on, when it named one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task: Option<TaskLink>,
        /// Absent on legacy lines, which are worktree requests.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<RequestOrigin>,
    },
    Proposal {
        request_id: String,
        proposal_id: String,
        body: String,
        rationale: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<String>,
    },
    /// A note for the operator about a request. Never delivered (BR-07).
    Advice {
        request_id: String,
        body: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<String>,
    },
    TriageFailed {
        request_id: String,
        error: String,
    },
    Rejected {
        request_id: String,
        reason: String,
    },
    ResolutionConfirmed {
        request_id: String,
        /// `None` when the operator authored the resolution (UC-06c).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        proposal_id: Option<String>,
        content_hash: String,
        text: String,
        edited: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<String>,
    },
    Delivered {
        request_id: String,
        attempt: u32,
    },
    DeliveryFailed {
        request_id: String,
        attempt: u32,
        error: String,
    },
    /// An operator comment on a worktree thread reached that worktree's agent
    /// pane. Its parent is the comment.
    CommentDelivered {
        attempt: u32,
    },
    /// Pasting an operator comment failed; the comment itself is kept. Its
    /// parent is the comment. Older binaries skip this line when reading.
    CommentDeliveryFailed {
        attempt: u32,
        error: String,
    },
    PriorityChanged {
        from: Priority,
        to: Priority,
        source: PrioritySource,
    },
    Redacted {
        target_event_id: String,
    },
    /// A note was written by the operator or the coordinator. The note text
    /// stays in the draft; the hash drives the `coordinator` badge.
    NoteUpserted {
        note_id: String,
        by: NoteAuthor,
        note_hash: String,
    },
    /// One coordinator chat turn. The text lives in `<id>.coordinator.jsonl`.
    CoordinatorTurn {
        turn_id: String,
        agent: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        session_id: String,
        transcript_offset: u64,
    },
    SolutionPhaseChanged {
        phase: SolutionPhase,
    },
    /// Operator only: the one thing that resets a scope's round count.
    ReviewCycleStarted {
        level: ReviewLevel,
        scope: String,
        cycle: u32,
    },
    /// Written only by the review gate; unique on `round`.
    ReviewRoundRecorded {
        round: RoundRef,
        verdict: ReviewVerdict,
        reviewer: AgentProfile,
        /// document -> SHA-256 of what was reviewed.
        #[serde(default)]
        doc_hashes: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        problem_ids: Vec<String>,
    },
    /// A review round thrown away because the reviewer changed the snapshot
    /// or the live worktree (FT-04). It consumes no round.
    ReviewRoundVoided {
        round: RoundRef,
        reason: String,
    },
    /// One escalated problem, carried as its own request (FR-18). A new kind,
    /// so an older binary skips it instead of folding a request with no worktree.
    ProblemRaised {
        request_id: String,
        problem_id: String,
        round: RoundRef,
        title: String,
        detail: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<String>,
    },
    /// Operator only. Binds to the SHA-256 of every design document.
    DesignApproved {
        doc_hashes: BTreeMap<String, String>,
        credential: CredentialClass,
    },
    HandoverDrafted {
        project: String,
        instruction_hash: String,
        design_hash: String,
        branch: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<String>,
    },
    /// Operator only. Approves one project's worktree for exactly this text.
    HandoverApproved {
        project: String,
        instruction_hash: String,
        design_hash: String,
        branch: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<String>,
        credential: CredentialClass,
    },
    HandoverDeferred {
        project: String,
        credential: CredentialClass,
    },
    /// The operator dismissed a request (track sebenza_polishing). One line
    /// carries the status change, the outbox entry for the dismissal notice
    /// and the redelivery cancel, so a dismissal is all or nothing.
    RequestDismissed {
        request_id: String,
        from: RequestStatus,
        /// The request was `delivery_failed`: no further answer redelivery.
        #[serde(default)]
        cancels_redelivery: bool,
        credential: CredentialClass,
        /// The notice outbox entry; absent when no pane receives the request.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notice: Option<NoticeEnqueued>,
    },
    /// The operator archived a confirmed or resolved request. Status unchanged.
    RequestArchived {
        request_id: String,
        status_at_archive: RequestStatus,
        credential: CredentialClass,
    },
    /// One attempt to deliver a request notice. `started` is written before
    /// the send and `sent`/`failed` after it, so an attempt interrupted by a
    /// crash is visible as a `started` with no outcome.
    NoticeAttempt {
        request_id: String,
        attempt: u32,
        trigger: NoticeTrigger,
        outcome: NoticeOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// The startup backfill saw this request (actor `system:migration`).
    RequestMigrated {
        request_id: String,
        status: RequestStatus,
    },
    /// An event type written by a newer binary. Kept out of every fold.
    #[serde(other)]
    Unknown,
}

/// The outbox entry a dismissal carries. `key` is `request_id:dismissal`, the
/// idempotency key the notice text quotes so an agent can drop a duplicate.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct NoticeEnqueued {
    pub kind: NoticeKind,
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    Dismissal,
}

/// Who started a notice attempt: the dispatcher's retry loop or the
/// operator's Retry action.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoticeTrigger {
    Dispatcher,
    Manual,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoticeOutcome {
    Started,
    Sent,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct InboxEvent {
    pub schema_version: i32,
    pub event_id: String,
    pub ts: String,
    pub author: AuthorKind,
    /// Self-declared caller marker (`"worktree"`), recorded but never trusted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<String>,
    #[serde(flatten)]
    pub kind: InboxEventKind,
}

/// Request lifecycle, per the design's state table.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequestStatus {
    Open,
    Proposed,
    Confirmed,
    Resolved,
    DeliveryFailed,
    /// Dismissed by the operator. Terminal: later events never move it.
    Dismissed,
}

/// The current state of one request, folded from the log.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestView {
    pub request_id: String,
    /// The worktree that asked. `None` for a solution problem, which no pane
    /// receives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeKey>,
    pub origin: RequestOrigin,
    /// The escalated problem this request carries, for `SolutionProblem`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem_id: Option<String>,
    pub title: String,
    pub body: String,
    pub status: RequestStatus,
    /// Triage or delivery failed and the request needs the operator.
    pub flagged: bool,
    pub proposal_id: Option<String>,
    pub proposal: Option<String>,
    /// [`content_hash`] of `proposal`: what a confirm of it must quote.
    pub proposal_hash: Option<String>,
    /// Secret/PHI scan hits on the proposal, shown before confirm (FR-10).
    pub proposal_warnings: Vec<String>,
    /// Hash of the confirmed text, set once confirmed.
    pub content_hash: Option<String>,
    pub confirmed_text: Option<String>,
    pub last_reason: Option<String>,
    pub last_error: Option<String>,
    /// Number of delivery attempts recorded so far.
    pub attempts: u32,
    pub warnings: Vec<String>,
    pub opened_at: String,
    /// The plan task this request blocks, as the agent named it.
    pub task: Option<TaskLink>,
    /// Set by an archive; archiving never changes `status`.
    pub archived_at: Option<String>,
    pub archived_by: Option<String>,
    pub dismissed_at: Option<String>,
    pub dismissed_by: Option<String>,
    /// Template summaries of the request and its result (title only).
    pub summary: super::request_lifecycle::RequestSummary,
    /// The dismissal notice outbox entry, when there is one.
    pub notice: Option<super::request_lifecycle::NoticeView>,
}

/// A plan task a request is about: `track_id` and `task_id` from the
/// worktree's `plan.json`. Self-declared by the agent, so display-only.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskLink {
    pub track_id: String,
    pub task_id: String,
}

/// Fold the log into one view per request, in the order requests were opened.
/// Events naming an unknown request are ignored.
pub fn fold_requests(events: &[InboxEvent]) -> Vec<RequestView> {
    fold_requests_with(
        events,
        super::request_lifecycle::DEFAULT_NOTICE_MAX_ATTEMPTS,
    )
}

/// [`fold_requests`] with the configured `notify.maxAttempts`, which decides
/// when a dismissal notice counts as `failed`.
pub fn fold_requests_with(events: &[InboxEvent], max_notice_attempts: u32) -> Vec<RequestView> {
    let mut views: Vec<RequestView> = Vec::new();
    for e in events {
        let opened = match &e.kind {
            InboxEventKind::RequestOpened {
                request_id,
                worktree,
                title,
                body,
                warnings,
                task,
                origin,
            } => Some(RequestView::opened(
                request_id,
                Some(worktree.clone()),
                origin.unwrap_or_default(),
                None,
                title,
                body,
                warnings,
                &e.ts,
                task.clone(),
            )),
            InboxEventKind::ProblemRaised {
                request_id,
                problem_id,
                title,
                detail,
                warnings,
                ..
            } => Some(RequestView::opened(
                request_id,
                None,
                RequestOrigin::SolutionProblem,
                Some(problem_id.clone()),
                title,
                detail,
                warnings,
                &e.ts,
                None,
            )),
            _ => None,
        };
        if let Some(view) = opened {
            if views.iter().all(|v| v.request_id != view.request_id) {
                views.push(view);
            }
            continue;
        }
        let Some(rid) = request_id_of(&e.kind) else {
            continue;
        };
        let Some(v) = views.iter_mut().find(|v| v.request_id == rid) else {
            continue;
        };
        apply_request_event(v, e, max_notice_attempts);
    }
    for v in &mut views {
        v.summary = super::request_lifecycle::summarize(v);
    }
    views
}

/// Apply one event that names an existing request. Shared by the fold and the
/// history projection, so both read the same state machine.
pub(crate) fn apply_request_event(v: &mut RequestView, e: &InboxEvent, max_notice_attempts: u32) {
    // Dismissed is terminal: late answers, deliveries and triage stay in the
    // history but never move the request again.
    let live = v.status != RequestStatus::Dismissed;
    match &e.kind {
        // A late proposal never reopens a decided request.
        InboxEventKind::Proposal {
            proposal_id,
            body,
            warnings,
            ..
        } if matches!(v.status, RequestStatus::Open | RequestStatus::Proposed) => {
            v.status = RequestStatus::Proposed;
            v.flagged = false;
            v.proposal_id = Some(proposal_id.clone());
            v.proposal = Some(body.clone());
            v.proposal_hash = Some(content_hash(body));
            v.proposal_warnings = warnings.clone();
        }
        InboxEventKind::TriageFailed { error, .. } if live => {
            v.flagged = true;
            v.last_error = Some(error.clone());
        }
        InboxEventKind::Rejected { reason, .. } if live => {
            v.status = RequestStatus::Open;
            v.proposal_id = None;
            v.proposal = None;
            v.proposal_hash = None;
            v.proposal_warnings = Vec::new();
            v.last_reason = Some(reason.clone());
        }
        InboxEventKind::ResolutionConfirmed {
            content_hash, text, ..
        } if live => {
            v.status = RequestStatus::Confirmed;
            v.flagged = false;
            v.content_hash = Some(content_hash.clone());
            v.confirmed_text = Some(text.clone());
        }
        InboxEventKind::Delivered { attempt, .. } if live => {
            v.status = RequestStatus::Resolved;
            v.flagged = false;
            v.attempts = v.attempts.max(*attempt);
        }
        InboxEventKind::DeliveryFailed { attempt, error, .. } if live => {
            v.status = RequestStatus::DeliveryFailed;
            v.flagged = true;
            v.attempts = v.attempts.max(*attempt);
            v.last_error = Some(error.clone());
        }
        InboxEventKind::RequestDismissed {
            credential, notice, ..
        } if live => {
            v.status = RequestStatus::Dismissed;
            v.flagged = false;
            v.dismissed_at = Some(e.ts.clone());
            v.dismissed_by = Some(credential.as_str().to_string());
            v.notice = notice
                .as_ref()
                .map(|n| super::request_lifecycle::NoticeView::enqueued(&n.key));
        }
        InboxEventKind::RequestArchived { credential, .. }
            if v.archived_at.is_none()
                && matches!(v.status, RequestStatus::Confirmed | RequestStatus::Resolved) =>
        {
            v.archived_at = Some(e.ts.clone());
            v.archived_by = Some(credential.as_str().to_string());
        }
        InboxEventKind::NoticeAttempt {
            attempt,
            trigger,
            outcome,
            error,
            ..
        } => {
            if let Some(n) = v.notice.as_mut() {
                n.record(*attempt, *trigger, *outcome, error.as_deref(), &e.ts);
            }
        }
        _ => {}
    }
    if let Some(n) = v.notice.as_mut() {
        n.settle(max_notice_attempts);
    }
}

impl RequestView {
    #[allow(clippy::too_many_arguments)]
    fn opened(
        request_id: &str,
        worktree: Option<WorktreeKey>,
        origin: RequestOrigin,
        problem_id: Option<String>,
        title: &str,
        body: &str,
        warnings: &[String],
        ts: &str,
        task: Option<TaskLink>,
    ) -> Self {
        RequestView {
            request_id: request_id.to_string(),
            worktree,
            origin,
            problem_id,
            title: title.to_string(),
            body: body.to_string(),
            status: RequestStatus::Open,
            flagged: false,
            proposal_id: None,
            proposal: None,
            proposal_hash: None,
            proposal_warnings: Vec::new(),
            content_hash: None,
            confirmed_text: None,
            last_reason: None,
            last_error: None,
            attempts: 0,
            warnings: warnings.to_vec(),
            opened_at: ts.to_string(),
            task,
            archived_at: None,
            archived_by: None,
            dismissed_at: None,
            dismissed_by: None,
            summary: Default::default(),
            notice: None,
        }
    }

    /// Whether a confirmed answer is pasted into a worktree pane. A solution
    /// problem's answer goes to the coordinator session instead (TS-53).
    pub fn delivers_to_pane(&self) -> bool {
        self.worktree.is_some() && self.origin != RequestOrigin::SolutionProblem
    }
}

/// The request an event belongs to, if any.
pub fn request_id_of(kind: &InboxEventKind) -> Option<&str> {
    match kind {
        InboxEventKind::RequestOpened { request_id, .. }
        | InboxEventKind::ProblemRaised { request_id, .. }
        | InboxEventKind::Proposal { request_id, .. }
        | InboxEventKind::Advice { request_id, .. }
        | InboxEventKind::TriageFailed { request_id, .. }
        | InboxEventKind::Rejected { request_id, .. }
        | InboxEventKind::ResolutionConfirmed { request_id, .. }
        | InboxEventKind::Delivered { request_id, .. }
        | InboxEventKind::DeliveryFailed { request_id, .. }
        | InboxEventKind::RequestDismissed { request_id, .. }
        | InboxEventKind::RequestArchived { request_id, .. }
        | InboxEventKind::NoticeAttempt { request_id, .. }
        | InboxEventKind::RequestMigrated { request_id, .. } => Some(request_id),
        _ => None,
    }
}

/// The text shown in place of a redacted body.
pub const REDACTED_BODY: &str = "[redacted]";

/// Replace the body of every event targeted by a `redacted` tombstone.
pub fn apply_redactions(events: &mut [InboxEvent]) {
    let targets: std::collections::HashSet<String> = events
        .iter()
        .filter_map(|e| match &e.kind {
            InboxEventKind::Redacted { target_event_id } => Some(target_event_id.clone()),
            _ => None,
        })
        .collect();
    for e in events.iter_mut().filter(|e| targets.contains(&e.event_id)) {
        match &mut e.kind {
            InboxEventKind::Comment { body, .. }
            | InboxEventKind::RequestOpened { body, .. }
            | InboxEventKind::Proposal { body, .. }
            | InboxEventKind::Advice { body, .. } => *body = REDACTED_BODY.to_string(),
            _ => {}
        }
    }
}

/// The item's system-agent session, stored server-side in `<ulid>.session.json`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct AgentSession {
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub session_id: String,
    /// Turns run on this session; past the configured cap it is re-seeded.
    #[serde(default)]
    pub turns: u32,
}

/// The SHA-1 hex of `text`, used to bind a confirm to the exact text shown.
pub fn content_hash(text: &str) -> String {
    super::model::FileRevision::of_body(text).body_hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: &str, kind: InboxEventKind) -> InboxEvent {
        InboxEvent {
            schema_version: INBOX_EVENT_SCHEMA_VERSION,
            event_id: id.to_string(),
            ts: format!("2026-09-30T00:00:{id:0>2}Z"),
            author: AuthorKind::Operator,
            caller: None,
            parent_event_id: None,
            kind,
        }
    }

    fn wt() -> WorktreeKey {
        WorktreeKey {
            project: "/p/acme-demo".into(),
            branch: "feat-x".into(),
        }
    }

    fn opened(id: &str, rid: &str) -> InboxEvent {
        ev(
            id,
            InboxEventKind::RequestOpened {
                request_id: rid.into(),
                worktree: wt(),
                title: "Need a decision".into(),
                body: "Which db?".into(),
                warnings: vec![],
                task: None,
                origin: None,
            },
        )
    }

    fn status(events: &[InboxEvent]) -> RequestStatus {
        fold_requests(events)[0].status
    }

    #[test]
    fn fold_follows_the_state_table() {
        let mut log = vec![opened("1", "r1")];
        assert_eq!(status(&log), RequestStatus::Open);

        log.push(ev(
            "2",
            InboxEventKind::Proposal {
                request_id: "r1".into(),
                proposal_id: "p1".into(),
                body: "Use sqlite".into(),
                rationale: "simple".into(),
                warnings: vec![],
            },
        ));
        assert_eq!(status(&log), RequestStatus::Proposed);

        log.push(ev(
            "3",
            InboxEventKind::Rejected {
                request_id: "r1".into(),
                reason: "no".into(),
            },
        ));
        let v = &fold_requests(&log)[0];
        assert_eq!(v.status, RequestStatus::Open);
        assert_eq!(v.last_reason.as_deref(), Some("no"));
        assert_eq!(v.proposal, None, "a rejected proposal is no longer pending");

        log.push(ev(
            "4",
            InboxEventKind::ResolutionConfirmed {
                request_id: "r1".into(),
                proposal_id: None,
                content_hash: content_hash("Use postgres"),
                text: "Use postgres".into(),
                edited: false,
                warnings: vec![],
            },
        ));
        assert_eq!(status(&log), RequestStatus::Confirmed);

        log.push(ev(
            "5",
            InboxEventKind::DeliveryFailed {
                request_id: "r1".into(),
                attempt: 1,
                error: "pane gone".into(),
            },
        ));
        let v = &fold_requests(&log)[0];
        assert_eq!(v.status, RequestStatus::DeliveryFailed);
        assert!(v.flagged);
        assert_eq!(v.attempts, 1);

        log.push(ev(
            "6",
            InboxEventKind::Delivered {
                request_id: "r1".into(),
                attempt: 2,
            },
        ));
        let v = &fold_requests(&log)[0];
        assert_eq!(v.status, RequestStatus::Resolved);
        assert!(!v.flagged);
        assert_eq!(v.attempts, 2);
        assert_eq!(v.confirmed_text.as_deref(), Some("Use postgres"));
    }

    #[test]
    fn triage_failure_flags_but_keeps_request_open() {
        let log = vec![
            opened("1", "r1"),
            ev(
                "2",
                InboxEventKind::TriageFailed {
                    request_id: "r1".into(),
                    error: "timeout".into(),
                },
            ),
        ];
        let v = &fold_requests(&log)[0];
        assert_eq!(v.status, RequestStatus::Open);
        assert!(v.flagged);
        assert_eq!(v.last_error.as_deref(), Some("timeout"));
    }

    #[test]
    fn fold_ignores_unknown_requests_and_event_types() {
        let log = vec![
            ev("1", InboxEventKind::Unknown),
            ev(
                "2",
                InboxEventKind::Delivered {
                    request_id: "ghost".into(),
                    attempt: 1,
                },
            ),
            opened("3", "r1"),
            opened("4", "r2"),
        ];
        let views = fold_requests(&log);
        assert_eq!(
            views
                .iter()
                .map(|v| v.request_id.as_str())
                .collect::<Vec<_>>(),
            ["r1", "r2"]
        );
    }

    #[test]
    fn unknown_event_type_deserializes_as_unknown() {
        let line = r#"{"schema_version":1,"event_id":"x","ts":"t","author":"operator","type":"from_the_future","extra":1}"#;
        let e: InboxEvent = serde_json::from_str(line).expect("parse");
        assert_eq!(e.kind, InboxEventKind::Unknown);
    }

    #[test]
    fn redaction_masks_target_body_only() {
        let mut log = vec![
            ev(
                "1",
                InboxEventKind::Comment {
                    thread: Thread::Worktree(wt()),
                    body: "token sk-TEST-0000".into(),
                    warnings: vec!["generic-api-key".into()],
                },
            ),
            ev(
                "2",
                InboxEventKind::Comment {
                    thread: Thread::Overall,
                    body: "fine".into(),
                    warnings: vec![],
                },
            ),
            ev(
                "3",
                InboxEventKind::Redacted {
                    target_event_id: "1".into(),
                },
            ),
        ];
        apply_redactions(&mut log);
        match &log[0].kind {
            InboxEventKind::Comment { body, .. } => assert_eq!(body, REDACTED_BODY),
            other => panic!("{other:?}"),
        }
        match &log[1].kind {
            InboxEventKind::Comment { body, .. } => assert_eq!(body, "fine"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn event_wire_shape_is_flat_and_tagged() {
        let e = opened("1", "r1");
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert_eq!(v["type"], "request_opened");
        assert_eq!(v["author"], "operator");
        assert_eq!(v["worktree"]["branch"], "feat-x");
        let back: InboxEvent = serde_json::from_value(v).unwrap();
        assert_eq!(back, e);
    }

    // FR-07: every factory event has a stable snake_case wire name, round-trips,
    // and reads as `Unknown` to a binary that predates it.
    #[test]
    fn factory_events_have_stable_wire_names() {
        use crate::domain::solution::*;
        let r = RoundRef {
            level: ReviewLevel::ProjectDesign,
            scope: "/p@b/t".into(),
            cycle: 1,
            round: 2,
        };
        let cases = vec![
            (
                "note_upserted",
                InboxEventKind::NoteUpserted {
                    note_id: "n".into(),
                    by: NoteAuthor::Coordinator,
                    note_hash: "h".into(),
                },
            ),
            (
                "coordinator_turn",
                InboxEventKind::CoordinatorTurn {
                    turn_id: "t".into(),
                    agent: "codex".into(),
                    model: Some("gpt-5".into()),
                    session_id: "s".into(),
                    transcript_offset: 42,
                },
            ),
            (
                "solution_phase_changed",
                InboxEventKind::SolutionPhaseChanged {
                    phase: SolutionPhase::Reviewing,
                },
            ),
            (
                "review_cycle_started",
                InboxEventKind::ReviewCycleStarted {
                    level: ReviewLevel::Code,
                    scope: "s".into(),
                    cycle: 2,
                },
            ),
            (
                "review_round_recorded",
                InboxEventKind::ReviewRoundRecorded {
                    round: r.clone(),
                    verdict: ReviewVerdict::Changes,
                    reviewer: AgentProfile::new("claude", Some("fable")),
                    doc_hashes: BTreeMap::from([("design.md".into(), "h".into())]),
                    problem_ids: vec!["p1".into()],
                },
            ),
            (
                "problem_raised",
                InboxEventKind::ProblemRaised {
                    request_id: "r".into(),
                    problem_id: "p".into(),
                    round: r,
                    title: "t".into(),
                    detail: "d".into(),
                    warnings: vec![],
                },
            ),
            (
                "design_approved",
                InboxEventKind::DesignApproved {
                    doc_hashes: BTreeMap::from([("design.md".into(), "h".into())]),
                    credential: CredentialClass::OperatorCli,
                },
            ),
            (
                "handover_drafted",
                InboxEventKind::HandoverDrafted {
                    project: "/p".into(),
                    instruction_hash: "i".into(),
                    design_hash: "d".into(),
                    branch: "b".into(),
                    base: None,
                },
            ),
            (
                "handover_approved",
                InboxEventKind::HandoverApproved {
                    project: "/p".into(),
                    instruction_hash: "i".into(),
                    design_hash: "d".into(),
                    branch: "b".into(),
                    base: Some("main".into()),
                    credential: CredentialClass::OperatorSession,
                },
            ),
            (
                "handover_deferred",
                InboxEventKind::HandoverDeferred {
                    project: "/p".into(),
                    credential: CredentialClass::OperatorSession,
                },
            ),
        ];
        for (name, kind) in cases {
            let e = ev("1", kind);
            let v = serde_json::to_value(&e).unwrap();
            assert_eq!(v["type"], name);
            assert_eq!(
                v["author"], "operator",
                "{name}: the flattened kind must not shadow author"
            );
            let back: InboxEvent = serde_json::from_value(v.clone()).unwrap();
            assert_eq!(back, e, "{name} round-trips");
            // What an older binary sees: a type it does not know.
            let mut old = v;
            old["type"] = serde_json::json!(format!("{name}_v99"));
            let seen: InboxEvent = serde_json::from_value(old).unwrap();
            assert_eq!(seen.kind, InboxEventKind::Unknown);
        }
    }

    #[test]
    fn request_origin_defaults_to_worktree_and_round_trips() {
        use crate::domain::solution::RequestOrigin;
        let legacy = r#"{"schema_version":1,"event_id":"1","ts":"t","author":"worktree_agent","type":"request_opened","request_id":"r","worktree":{"project":"/p","branch":"b"},"title":"t","body":"b"}"#;
        let e: InboxEvent = serde_json::from_str(legacy).unwrap();
        let v = &fold_requests(std::slice::from_ref(&e))[0];
        assert_eq!(v.origin, RequestOrigin::Worktree);
        assert!(v.delivers_to_pane());
        assert!(
            !serde_json::to_string(&e).unwrap().contains("origin"),
            "an unset origin is not written"
        );

        let mut approval = opened("2", "r2");
        if let InboxEventKind::RequestOpened { origin, .. } = &mut approval.kind {
            *origin = Some(RequestOrigin::Approval);
        }
        let json = serde_json::to_string(&approval).unwrap();
        assert!(json.contains(r#""origin":"approval""#));
        let v = &fold_requests(&[serde_json::from_str(&json).unwrap()])[0];
        assert_eq!(v.origin, RequestOrigin::Approval);
        assert_eq!(v.worktree, Some(wt()));
    }
}
