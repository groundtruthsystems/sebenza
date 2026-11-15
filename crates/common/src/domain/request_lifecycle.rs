//! Request lifecycle helpers (track sebenza_polishing): template summaries,
//! the Requests-tab filters, the dismissal-notice outbox state and the
//! per-request history projection. Everything here is derived from the
//! append-only event log; nothing is stored.

use super::inbox_events::{
    InboxEvent, InboxEventKind, NoticeOutcome, NoticeTrigger, RequestStatus, RequestView,
    apply_request_event, fold_requests_with, request_id_of,
};
use serde::Serialize;

/// `notify.maxAttempts` when the config does not say.
pub const DEFAULT_NOTICE_MAX_ATTEMPTS: u32 = 6;
/// The longest a summary line may be, in characters.
pub const SUMMARY_MAX_CHARS: usize = 140;

/// Short, deterministic summaries of a request and its result.
#[derive(Debug, Serialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestSummary {
    pub request: String,
    /// `None` while the request is open: there is no result yet.
    pub result: Option<String>,
}

/// The template summary. Title only, never the body, which is where agent
/// pasted secrets are most likely to sit.
pub fn summarize(v: &RequestView) -> RequestSummary {
    let source = match &v.worktree {
        Some(w) => w.branch.clone(),
        None => "solution".to_string(),
    };
    let request = clip(&format!("{}: {}", clean(&source), clean(&v.title)));
    let result = match v.status {
        RequestStatus::Open => None,
        RequestStatus::Proposed => Some("Answer proposed".to_string()),
        RequestStatus::Confirmed => Some("Answer confirmed, delivering".to_string()),
        RequestStatus::Resolved => Some("Answer delivered".to_string()),
        RequestStatus::DeliveryFailed => Some("Answer delivery failed".to_string()),
        RequestStatus::Dismissed => Some("Dismissed by the operator".to_string()),
    };
    RequestSummary { request, result }
}

fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn clip(s: &str) -> String {
    if s.chars().count() <= SUMMARY_MAX_CHARS {
        return s.to_string();
    }
    let mut out: String = s.chars().take(SUMMARY_MAX_CHARS - 1).collect();
    out.push('…');
    out
}

/// The Requests-tab filters. There is deliberately no Archived filter (v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestFilter {
    Unanswered,
    Answered,
    Dismissed,
    #[default]
    All,
}

impl RequestFilter {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "unanswered" => Some(Self::Unanswered),
            "answered" => Some(Self::Answered),
            "dismissed" => Some(Self::Dismissed),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    /// Unanswered = open, proposed, delivery_failed; Answered = confirmed,
    /// resolved; Dismissed = dismissed; All = everything, archived or not.
    pub fn matches(self, status: RequestStatus) -> bool {
        use RequestStatus::*;
        match self {
            Self::Unanswered => matches!(status, Open | Proposed | DeliveryFailed),
            Self::Answered => matches!(status, Confirmed | Resolved),
            Self::Dismissed => status == Dismissed,
            Self::All => true,
        }
    }
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoticeState {
    Pending,
    Sent,
    Failed,
}

/// The dismissal notice's outbox state, folded from `notice_attempt` events.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoticeView {
    /// `request_id:dismissal`.
    pub key: String,
    pub state: NoticeState,
    /// Dispatcher attempts that did not succeed (failed or interrupted).
    pub failures: u32,
    /// The highest attempt number recorded.
    pub last_attempt: u32,
    pub last_attempt_at: Option<String>,
    pub last_error: Option<String>,
    #[serde(skip)]
    attempts: Vec<AttemptRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttemptRecord {
    attempt: u32,
    trigger: NoticeTrigger,
    outcome: Option<NoticeOutcome>,
}

impl NoticeView {
    pub fn enqueued(key: &str) -> Self {
        Self {
            key: key.to_string(),
            state: NoticeState::Pending,
            failures: 0,
            last_attempt: 0,
            last_attempt_at: None,
            last_error: None,
            attempts: Vec::new(),
        }
    }

    pub(crate) fn record(
        &mut self,
        attempt: u32,
        trigger: NoticeTrigger,
        outcome: NoticeOutcome,
        error: Option<&str>,
        ts: &str,
    ) {
        self.last_attempt = self.last_attempt.max(attempt);
        self.last_attempt_at = Some(ts.to_string());
        if let Some(err) = error {
            self.last_error = Some(err.to_string());
        }
        match self.attempts.iter_mut().find(|a| a.attempt == attempt) {
            Some(a) if outcome != NoticeOutcome::Started => a.outcome = Some(outcome),
            Some(_) => {}
            None => self.attempts.push(AttemptRecord {
                attempt,
                trigger,
                outcome: (outcome != NoticeOutcome::Started).then_some(outcome),
            }),
        }
    }

    /// Recompute `state` and `failures`. Sent wins. A manual Retry that did
    /// not succeed leaves the notice failed (the operator retries again);
    /// otherwise the dispatcher keeps trying until `max_attempts` failures.
    /// An attempt with no outcome was interrupted and counts as a failure,
    /// so the dispatcher resumes it after a restart.
    pub(crate) fn settle(&mut self, max_attempts: u32) {
        if self
            .attempts
            .iter()
            .any(|a| a.outcome == Some(NoticeOutcome::Sent))
        {
            self.state = NoticeState::Sent;
            return;
        }
        self.failures = self
            .attempts
            .iter()
            .filter(|a| a.trigger == NoticeTrigger::Dispatcher)
            .count() as u32;
        let manual = self
            .attempts
            .iter()
            .any(|a| a.trigger == NoticeTrigger::Manual);
        self.state = if manual || self.failures >= max_attempts.max(1) {
            NoticeState::Failed
        } else {
            NoticeState::Pending
        };
    }

    /// Seconds the dispatcher waits after the last failure before the next
    /// attempt: `min(base * 2^(failures-1), 900)`; zero before any attempt.
    pub fn backoff_secs(&self, base_secs: u64) -> u64 {
        if self.failures == 0 {
            return 0;
        }
        let exp = (self.failures - 1).min(20);
        base_secs.saturating_mul(1u64 << exp).min(900)
    }
}

/// One row of a request's history (`GET …/requests/{rid}/history`).
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// created | status_changed | dismissed | notice_enqueued | archived |
    /// notice_attempt | migrated
    pub kind: &'static str,
    pub from: Option<RequestStatus>,
    pub to: Option<RequestStatus>,
    pub actor: String,
    pub at: String,
    /// For `notice_attempt`: the trigger and outcome, e.g. `dispatcher:failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One answer on record: a proposal or the confirmed resolution.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryAnswer {
    pub kind: &'static str,
    pub text: String,
    pub at: String,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestHistory {
    pub request: RequestView,
    pub answers: Vec<HistoryAnswer>,
    pub events: Vec<HistoryEntry>,
}

fn actor(e: &InboxEvent) -> String {
    use super::inbox_events::AuthorKind;
    let credential = match &e.kind {
        InboxEventKind::RequestDismissed { credential, .. }
        | InboxEventKind::RequestArchived { credential, .. } => Some(credential.as_str()),
        _ => None,
    };
    match (e.author, credential, e.caller.as_deref()) {
        (AuthorKind::Operator, Some(c), _) => format!("operator:{c}"),
        (AuthorKind::Operator, None, Some(c)) => format!("operator:{c}"),
        (AuthorKind::Operator, None, None) => "operator".to_string(),
        (AuthorKind::WorktreeAgent, _, _) => "worktree_agent".to_string(),
        (AuthorKind::SystemAgent, _, Some(c)) if c.starts_with("system:") => c.to_string(),
        (AuthorKind::SystemAgent, _, _) => "system_agent".to_string(),
    }
}

/// The ordered history of one request. Replays the same state machine as the
/// fold, so every row's `from`/`to` matches what the list showed at the time.
pub fn request_history(
    events: &[InboxEvent],
    request_id: &str,
    max_notice_attempts: u32,
) -> Option<RequestHistory> {
    let request = fold_requests_with(events, max_notice_attempts)
        .into_iter()
        .find(|v| v.request_id == request_id)?;
    let mut rows = Vec::new();
    let mut answers = Vec::new();
    let mut view: Option<RequestView> = None;
    for e in events {
        if request_id_of(&e.kind) != Some(request_id) {
            continue;
        }
        let Some(v) = view.as_mut() else {
            // The opening event: fold just it to get the starting view.
            view = fold_requests_with(std::slice::from_ref(e), max_notice_attempts)
                .into_iter()
                .next();
            if view.is_some() {
                rows.push(HistoryEntry {
                    kind: "created",
                    from: None,
                    to: Some(RequestStatus::Open),
                    actor: actor(e),
                    at: e.ts.clone(),
                    detail: None,
                });
            }
            continue;
        };
        let before = v.status;
        apply_request_event(v, e, max_notice_attempts);
        let after = v.status;
        let row = |kind, from, to, detail| HistoryEntry {
            kind,
            from,
            to,
            actor: actor(e),
            at: e.ts.clone(),
            detail,
        };
        match &e.kind {
            InboxEventKind::Proposal { body, .. } => answers.push(HistoryAnswer {
                kind: "proposal",
                text: body.clone(),
                at: e.ts.clone(),
            }),
            InboxEventKind::ResolutionConfirmed { text, .. } => answers.push(HistoryAnswer {
                kind: "confirmed",
                text: text.clone(),
                at: e.ts.clone(),
            }),
            _ => {}
        }
        match &e.kind {
            InboxEventKind::RequestDismissed { notice, .. }
                if after == RequestStatus::Dismissed && before != after =>
            {
                rows.push(row("dismissed", Some(before), Some(after), None));
                if notice.is_some() {
                    rows.push(row("notice_enqueued", None, None, None));
                }
            }
            InboxEventKind::RequestArchived { .. } => {
                rows.push(row("archived", Some(after), Some(after), None))
            }
            InboxEventKind::NoticeAttempt {
                trigger, outcome, ..
            } => {
                let t = match trigger {
                    NoticeTrigger::Dispatcher => "dispatcher",
                    NoticeTrigger::Manual => "manual",
                };
                let o = match outcome {
                    NoticeOutcome::Started => "started",
                    NoticeOutcome::Sent => "sent",
                    NoticeOutcome::Failed => "failed",
                };
                rows.push(row("notice_attempt", None, None, Some(format!("{t}:{o}"))));
            }
            InboxEventKind::RequestMigrated { status, .. } => {
                rows.push(row("migrated", Some(*status), Some(*status), None))
            }
            _ if before != after => {
                rows.push(row("status_changed", Some(before), Some(after), None))
            }
            _ => {}
        }
    }
    Some(RequestHistory {
        request,
        answers,
        events: rows,
    })
}
