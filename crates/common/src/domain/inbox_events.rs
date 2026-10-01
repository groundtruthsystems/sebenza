//! Collaboration events for one inbox item, stored one JSON object per line in
//! `<ulid>.events.jsonl` beside the draft.
//!
//! Events are immutable. Request state is never stored: it is a fold over the
//! log ([`fold_requests`]), so the log is the single source of truth for
//! comments, requests, proposals, decisions and deliveries.

use super::model::{Priority, PrioritySource};
use serde::{Deserialize, Serialize};

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
    },
    Proposal {
        request_id: String,
        proposal_id: String,
        body: String,
        rationale: String,
    },
    Advice {
        request_id: String,
        body: String,
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
    PriorityChanged {
        from: Priority,
        to: Priority,
        source: PrioritySource,
    },
    Redacted {
        target_event_id: String,
    },
    /// An event type written by a newer binary. Kept out of every fold.
    #[serde(other)]
    Unknown,
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
}

/// The current state of one request, folded from the log.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestView {
    pub request_id: String,
    pub worktree: WorktreeKey,
    pub title: String,
    pub body: String,
    pub status: RequestStatus,
    /// Triage or delivery failed and the request needs the operator.
    pub flagged: bool,
    pub proposal_id: Option<String>,
    pub proposal: Option<String>,
    /// Hash of the confirmed text, set once confirmed.
    pub content_hash: Option<String>,
    pub confirmed_text: Option<String>,
    pub last_reason: Option<String>,
    pub last_error: Option<String>,
    /// Number of delivery attempts recorded so far.
    pub attempts: u32,
    pub warnings: Vec<String>,
    pub opened_at: String,
}

/// Fold the log into one view per request, in the order requests were opened.
/// Events naming an unknown request are ignored.
pub fn fold_requests(events: &[InboxEvent]) -> Vec<RequestView> {
    let mut views: Vec<RequestView> = Vec::new();
    for e in events {
        if let InboxEventKind::RequestOpened {
            request_id,
            worktree,
            title,
            body,
            warnings,
        } = &e.kind
        {
            if views.iter().all(|v| &v.request_id != request_id) {
                views.push(RequestView {
                    request_id: request_id.clone(),
                    worktree: worktree.clone(),
                    title: title.clone(),
                    body: body.clone(),
                    status: RequestStatus::Open,
                    flagged: false,
                    proposal_id: None,
                    proposal: None,
                    content_hash: None,
                    confirmed_text: None,
                    last_reason: None,
                    last_error: None,
                    attempts: 0,
                    warnings: warnings.clone(),
                    opened_at: e.ts.clone(),
                });
            }
            continue;
        }
        let Some(rid) = request_id_of(&e.kind) else {
            continue;
        };
        let Some(v) = views.iter_mut().find(|v| v.request_id == rid) else {
            continue;
        };
        match &e.kind {
            InboxEventKind::Proposal {
                proposal_id, body, ..
            } => {
                v.status = RequestStatus::Proposed;
                v.flagged = false;
                v.proposal_id = Some(proposal_id.clone());
                v.proposal = Some(body.clone());
            }
            InboxEventKind::TriageFailed { error, .. } => {
                v.flagged = true;
                v.last_error = Some(error.clone());
            }
            InboxEventKind::Rejected { reason, .. } => {
                v.status = RequestStatus::Open;
                v.proposal_id = None;
                v.proposal = None;
                v.last_reason = Some(reason.clone());
            }
            InboxEventKind::ResolutionConfirmed {
                content_hash, text, ..
            } => {
                v.status = RequestStatus::Confirmed;
                v.flagged = false;
                v.content_hash = Some(content_hash.clone());
                v.confirmed_text = Some(text.clone());
            }
            InboxEventKind::Delivered { attempt, .. } => {
                v.status = RequestStatus::Resolved;
                v.flagged = false;
                v.attempts = v.attempts.max(*attempt);
            }
            InboxEventKind::DeliveryFailed { attempt, error, .. } => {
                v.status = RequestStatus::DeliveryFailed;
                v.flagged = true;
                v.attempts = v.attempts.max(*attempt);
                v.last_error = Some(error.clone());
            }
            _ => {}
        }
    }
    views
}

/// The request an event belongs to, if any.
pub fn request_id_of(kind: &InboxEventKind) -> Option<&str> {
    match kind {
        InboxEventKind::RequestOpened { request_id, .. }
        | InboxEventKind::Proposal { request_id, .. }
        | InboxEventKind::Advice { request_id, .. }
        | InboxEventKind::TriageFailed { request_id, .. }
        | InboxEventKind::Rejected { request_id, .. }
        | InboxEventKind::ResolutionConfirmed { request_id, .. }
        | InboxEventKind::Delivered { request_id, .. }
        | InboxEventKind::DeliveryFailed { request_id, .. } => Some(request_id),
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
}
