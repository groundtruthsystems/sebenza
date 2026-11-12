//! The software factory's solution level: what kind of item a draft is, the
//! agent profiles it overrides, and the solution state folded from its event
//! log.
//!
//! Like requests ([`super::inbox_events::fold_requests`]), solution state is
//! never stored. [`fold_solution`] derives it from the item's
//! `<id>.events.jsonl`, so the log stays the single source of truth for review
//! rounds, problems, approvals and handovers.

use super::inbox_events::{InboxEvent, InboxEventKind, RequestStatus, fold_requests};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Review rounds per cycle, at every level (BR-03, BR-11). Not configurable.
pub const MAX_REVIEW_ROUNDS: u8 = 3;

/// What an inbox item is. An absent `kind` is a note, so every legacy draft
/// keeps its meaning.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DraftKind {
    #[default]
    Note,
    Solution,
}

/// One agent and, optionally, the model it runs. An unset model means the
/// agent's own default, which the independence rule cannot compare.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct AgentProfile {
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl AgentProfile {
    pub fn new(agent: &str, model: Option<&str>) -> Self {
        Self {
            agent: agent.to_string(),
            model: model.map(str::to_string),
        }
    }
}

/// The reviewer at each level, configured separately (FR-08).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct ReviewerProfiles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solution: Option<AgentProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<AgentProfile>,
}

impl ReviewerProfiles {
    pub fn is_empty(&self) -> bool {
        self.solution.is_none() && self.project.is_none()
    }
}

/// Per-item agent profiles in draft frontmatter (`factory:`). Each one set
/// here wins over the global `factory:` block in `sebenza.yaml`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct FactoryOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator: Option<AgentProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub designer: Option<AgentProfile>,
    #[serde(default, skip_serializing_if = "ReviewerProfiles::is_empty")]
    pub reviewer: ReviewerProfiles,
}

impl FactoryOverrides {
    pub fn is_empty(&self) -> bool {
        self.coordinator.is_none() && self.designer.is_none() && self.reviewer.is_empty()
    }
}

/// Where a solution item is in its lifecycle (design.md, class diagram).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SolutionPhase {
    #[default]
    Requirements,
    Designing,
    Reviewing,
    Revising,
    Escalated,
    Agreed,
    Approved,
    Handover,
}

impl SolutionPhase {
    /// Whether a `SolutionPhaseChanged` from `self` to `to` is allowed. The
    /// daemon checks this before writing; the fold ignores a change that fails it.
    pub fn can_move_to(self, to: SolutionPhase) -> bool {
        use SolutionPhase::*;
        matches!(
            (self, to),
            (Requirements, Designing)
                | (Designing, Reviewing)
                | (Reviewing, Revising | Agreed | Escalated)
                | (Revising, Reviewing)
                // A new review cycle after escalation or a later edit.
                | (Escalated | Agreed | Approved | Handover, Revising)
                | (Escalated | Agreed, Approved)
                | (Approved, Handover)
        )
    }
}

/// Which review a round belongs to.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ReviewLevel {
    Solution,
    ProjectDesign,
    Code,
}

impl ReviewLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            ReviewLevel::Solution => "solution",
            ReviewLevel::ProjectDesign => "project_design",
            ReviewLevel::Code => "code",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Agreed,
    Changes,
}

/// Who last wrote a note: shown as the note's badge while its hash matches.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoteAuthor {
    Operator,
    Coordinator,
}

/// Which credential authorised a gated action. Derived by the server from the
/// token presented, never from a self-declared header (FT-02).
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialClass {
    /// The SPA's same-origin operator session token.
    OperatorSession,
    /// A `sebenza-cli login` token the operator confirmed in the UI.
    OperatorCli,
    /// The shared control token agents also hold.
    Control,
}

/// Identifies one review round. `scope` is the item id for solution rounds and
/// [`project_scope`] for project rounds; it is persisted, never inferred.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct RoundRef {
    pub level: ReviewLevel,
    pub scope: String,
    pub cycle: u32,
    pub round: u8,
}

/// Where a request came from. Absent on legacy `RequestOpened` lines, which
/// are worktree requests.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RequestOrigin {
    #[default]
    Worktree,
    /// A skill asking the operator to approve a design before implementing.
    Approval,
    /// The final-verification request after code review.
    Verification,
    /// A problem the solution reviewer escalated (`ProblemRaised`). Answered
    /// into the coordinator session, never pasted into a pane.
    SolutionProblem,
}

/// The project-round scope key: `<project path>@<branch>/<track_id>`. A
/// re-created worktree with the same path, branch and track gets the same key,
/// so it cannot reset its round count (R-D5).
pub fn project_scope(project_path: &str, branch: &str, track_id: &str) -> String {
    format!("{project_path}@{branch}/{track_id}")
}

/// The SHA-256 hex of `text`. Approvals bind to it (SHA-1 stays for note
/// concurrency only).
pub fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// The key under which an event may be written at most once per item, or
/// `None` when repeats are allowed. The store appends a keyed event only if no
/// earlier event has the same key, so a double-click or a replayed approval
/// never records twice (FT-08).
pub fn idempotency_key(kind: &InboxEventKind) -> Option<String> {
    match kind {
        InboxEventKind::ReviewCycleStarted {
            level,
            scope,
            cycle,
        } => Some(format!("cycle:{}:{scope}:{cycle}", level.as_str())),
        InboxEventKind::ReviewRoundRecorded { round, .. } => Some(format!(
            "round:{}:{}:{}:{}",
            round.level.as_str(),
            round.scope,
            round.cycle,
            round.round
        )),
        InboxEventKind::ProblemRaised { problem_id, .. } => Some(format!("problem:{problem_id}")),
        InboxEventKind::DesignApproved { doc_hashes, .. } => {
            let set: Vec<String> = doc_hashes.iter().map(|(d, h)| format!("{d}={h}")).collect();
            Some(format!("design:{}", set.join(",")))
        }
        InboxEventKind::HandoverApproved {
            project,
            instruction_hash,
            ..
        } => Some(format!("handover:{project}:{instruction_hash}")),
        _ => None,
    }
}

/// The review cycle currently open for `(level, scope)`: the highest
/// `ReviewCycleStarted`, or 1 when none was started.
pub fn current_cycle(events: &[InboxEvent], level: ReviewLevel, scope: &str) -> u32 {
    events
        .iter()
        .filter_map(|e| match &e.kind {
            InboxEventKind::ReviewCycleStarted {
                level: l,
                scope: s,
                cycle,
            } if *l == level && s == scope => Some(*cycle),
            _ => None,
        })
        .max()
        .unwrap_or(1)
}

/// Rounds already recorded in the current cycle of `(level, scope)`.
pub fn rounds_used(events: &[InboxEvent], level: ReviewLevel, scope: &str) -> u8 {
    let cycle = current_cycle(events, level, scope);
    let rounds: std::collections::BTreeSet<u8> = events
        .iter()
        .filter_map(|e| match &e.kind {
            InboxEventKind::ReviewRoundRecorded { round, .. }
                if round.level == level && round.scope == scope && round.cycle == cycle =>
            {
                Some(round.round)
            }
            _ => None,
        })
        .collect();
    rounds.len() as u8
}

/// The round a new review of `(level, scope)` would record, or
/// `Err(MAX_REVIEW_ROUNDS)` when the cycle is used up (409 `round_limit`).
pub fn next_round(events: &[InboxEvent], level: ReviewLevel, scope: &str) -> Result<RoundRef, u8> {
    let used = rounds_used(events, level, scope);
    if used >= MAX_REVIEW_ROUNDS {
        return Err(MAX_REVIEW_ROUNDS);
    }
    Ok(RoundRef {
        level,
        scope: scope.to_string(),
        cycle: current_cycle(events, level, scope),
        round: used + 1,
    })
}

/// A recorded review round, as the solution view shows it.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoundView {
    pub round: RoundRef,
    pub verdict: ReviewVerdict,
    pub reviewer: AgentProfile,
    pub problem_ids: Vec<String>,
    pub at: String,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProblemStatus {
    Open,
    Resolved,
}

/// One escalated problem and the request that carries it (FR-18).
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProblemView {
    pub problem_id: String,
    pub request_id: String,
    pub round: RoundRef,
    pub title: String,
    pub status: ProblemStatus,
}

/// The operator's design approval: valid only while every document still
/// hashes to what was approved ([`SolutionState::design_approval_valid`]).
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesignApprovalView {
    pub doc_hashes: BTreeMap<String, String>,
    pub credential: CredentialClass,
    pub at: String,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HandoverStatus {
    Drafted,
    Approved,
    Deferred,
    Converted,
}

/// One affected project's handover instruction.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HandoverView {
    pub project: String,
    pub branch: String,
    pub base: Option<String>,
    pub instruction_hash: String,
    pub design_hash: String,
    pub status: HandoverStatus,
    /// The hash the operator approved, kept after a re-draft so a stale
    /// approval can be shown as void.
    pub approved_hash: Option<String>,
}

/// The solution level's current state, folded from the event log.
#[derive(Debug, Serialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SolutionState {
    pub phase: SolutionPhase,
    /// The open solution review cycle (1 until a new one is started).
    pub cycle: u32,
    /// Rounds recorded in that cycle, 0..=3.
    pub round: u8,
    pub rounds: Vec<RoundView>,
    pub problems: Vec<ProblemView>,
    pub design_approval: Option<DesignApprovalView>,
    pub handovers: Vec<HandoverView>,
    /// Coordinator turns recorded so far.
    pub coordinator_turns: u32,
    /// note id -> hash of the coordinator's latest write of that note. The
    /// `coordinator` badge shows while the note still has this hash.
    pub coordinator_notes: BTreeMap<String, String>,
}

impl SolutionState {
    /// Whether the design approval still binds: every approved document is
    /// present in `current` with the approved hash, and nothing was added.
    pub fn design_approval_valid(&self, current: &BTreeMap<String, String>) -> bool {
        self.design_approval
            .as_ref()
            .is_some_and(|a| &a.doc_hashes == current)
    }

    /// Whether `project`'s handover is approved for exactly `instruction_hash`.
    pub fn handover_approval_valid(&self, project: &str, instruction_hash: &str) -> bool {
        self.handovers
            .iter()
            .any(|h| h.project == project && h.approved_hash.as_deref() == Some(instruction_hash))
    }

    /// Problems still waiting for the operator.
    pub fn open_problems(&self) -> usize {
        self.problems
            .iter()
            .filter(|p| p.status == ProblemStatus::Open)
            .count()
    }
}

/// Fold the item's log into its solution state. `item_id` is the solution
/// review scope. Unknown events and phase changes the current phase does not
/// allow are ignored, so a corrupt or foreign line never wedges the item.
pub fn fold_solution(item_id: &str, events: &[InboxEvent]) -> SolutionState {
    let mut s = SolutionState {
        cycle: current_cycle(events, ReviewLevel::Solution, item_id),
        ..SolutionState::default()
    };
    for e in events {
        match &e.kind {
            InboxEventKind::NoteUpserted {
                note_id,
                by,
                note_hash,
            } => match by {
                NoteAuthor::Coordinator => {
                    s.coordinator_notes
                        .insert(note_id.clone(), note_hash.clone());
                }
                NoteAuthor::Operator => {
                    s.coordinator_notes.remove(note_id);
                }
            },
            InboxEventKind::CoordinatorTurn { .. } => s.coordinator_turns += 1,
            InboxEventKind::SolutionPhaseChanged { phase } => {
                if s.phase.can_move_to(*phase) {
                    s.phase = *phase;
                }
            }
            InboxEventKind::ReviewRoundRecorded {
                round,
                verdict,
                reviewer,
                problem_ids,
                ..
            } if round.level == ReviewLevel::Solution && round.scope == item_id => {
                if s.rounds.iter().any(|r| r.round == *round) {
                    continue;
                }
                s.rounds.push(RoundView {
                    round: round.clone(),
                    verdict: *verdict,
                    reviewer: reviewer.clone(),
                    problem_ids: problem_ids.clone(),
                    at: e.ts.clone(),
                });
            }
            InboxEventKind::ProblemRaised {
                request_id,
                problem_id,
                round,
                title,
                ..
            } => {
                if s.problems.iter().all(|p| &p.problem_id != problem_id) {
                    s.problems.push(ProblemView {
                        problem_id: problem_id.clone(),
                        request_id: request_id.clone(),
                        round: round.clone(),
                        title: title.clone(),
                        status: ProblemStatus::Open,
                    });
                }
            }
            InboxEventKind::DesignApproved {
                doc_hashes,
                credential,
            } => {
                s.design_approval = Some(DesignApprovalView {
                    doc_hashes: doc_hashes.clone(),
                    credential: *credential,
                    at: e.ts.clone(),
                });
            }
            InboxEventKind::HandoverDrafted {
                project,
                instruction_hash,
                design_hash,
                branch,
                base,
            } => {
                let h = handover_mut(&mut s.handovers, project);
                h.instruction_hash = instruction_hash.clone();
                h.design_hash = design_hash.clone();
                h.branch = branch.clone();
                h.base = base.clone();
                // A re-draft with new text voids an earlier approval of other text.
                h.status = if h.approved_hash.as_deref() == Some(instruction_hash.as_str()) {
                    HandoverStatus::Approved
                } else {
                    HandoverStatus::Drafted
                };
            }
            InboxEventKind::HandoverApproved {
                project,
                instruction_hash,
                design_hash,
                branch,
                base,
                ..
            } => {
                let h = handover_mut(&mut s.handovers, project);
                h.instruction_hash = instruction_hash.clone();
                h.design_hash = design_hash.clone();
                h.branch = branch.clone();
                h.base = base.clone();
                h.approved_hash = Some(instruction_hash.clone());
                h.status = HandoverStatus::Approved;
            }
            InboxEventKind::HandoverDeferred { project, .. } => {
                let h = handover_mut(&mut s.handovers, project);
                h.status = HandoverStatus::Deferred;
                h.approved_hash = None;
            }
            _ => {}
        }
    }
    s.round = s.rounds.iter().filter(|r| r.round.cycle == s.cycle).count() as u8;

    // Problems resolve through the request flow they ride on.
    let requests = fold_requests(events);
    for p in &mut s.problems {
        let resolved = requests.iter().any(|r| {
            r.request_id == p.request_id
                && matches!(r.status, RequestStatus::Confirmed | RequestStatus::Resolved)
        });
        if resolved {
            p.status = ProblemStatus::Resolved;
        }
    }
    s
}

/// Mark handovers whose worktree was created. `converted` comes from the
/// draft's `conversions[]`, not from events: pass the project paths there.
pub fn mark_converted(state: &mut SolutionState, converted_projects: &[String]) {
    for h in &mut state.handovers {
        if h.status == HandoverStatus::Approved && converted_projects.contains(&h.project) {
            h.status = HandoverStatus::Converted;
        }
    }
}

fn handover_mut<'a>(list: &'a mut Vec<HandoverView>, project: &str) -> &'a mut HandoverView {
    if let Some(i) = list.iter().position(|h| h.project == project) {
        return &mut list[i];
    }
    list.push(HandoverView {
        project: project.to_string(),
        branch: String::new(),
        base: None,
        instruction_hash: String::new(),
        design_hash: String::new(),
        status: HandoverStatus::Drafted,
        approved_hash: None,
    });
    list.last_mut().expect("just pushed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::inbox_events::{
        AuthorKind, INBOX_EVENT_SCHEMA_VERSION, InboxEvent, InboxEventKind, RequestView,
        content_hash,
    };

    const ITEM: &str = "01ITEM";

    fn ev(n: usize, kind: InboxEventKind) -> InboxEvent {
        InboxEvent {
            schema_version: INBOX_EVENT_SCHEMA_VERSION,
            event_id: format!("e{n:03}"),
            ts: format!("2026-11-11T00:00:{:02}Z", n % 60),
            author: AuthorKind::Operator,
            caller: None,
            parent_event_id: None,
            kind,
        }
    }

    fn round(level: ReviewLevel, scope: &str, cycle: u32, n: u8) -> RoundRef {
        RoundRef {
            level,
            scope: scope.into(),
            cycle,
            round: n,
        }
    }

    fn recorded(r: RoundRef, verdict: ReviewVerdict, problems: &[&str]) -> InboxEventKind {
        InboxEventKind::ReviewRoundRecorded {
            round: r,
            verdict,
            reviewer: AgentProfile::new("codex", None),
            doc_hashes: BTreeMap::from([("design.md".into(), sha256_hex("d"))]),
            problem_ids: problems.iter().map(|p| p.to_string()).collect(),
        }
    }

    fn phase(p: SolutionPhase) -> InboxEventKind {
        InboxEventKind::SolutionPhaseChanged { phase: p }
    }

    fn hashes(design: &str) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("design.md".to_string(), sha256_hex(design)),
            ("security-architecture.md".to_string(), sha256_hex("sec")),
        ])
    }

    #[test]
    fn sha256_matches_the_known_vector() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn phases_follow_the_lifecycle_and_the_fold_ignores_illegal_moves() {
        use SolutionPhase::*;
        assert!(Requirements.can_move_to(Designing));
        assert!(Reviewing.can_move_to(Escalated));
        assert!(Escalated.can_move_to(Approved));
        assert!(Approved.can_move_to(Handover));
        assert!(
            !Requirements.can_move_to(Approved),
            "no approval before design"
        );
        assert!(
            !Reviewing.can_move_to(Approved),
            "approval follows agreed or escalated"
        );
        assert!(!Designing.can_move_to(Handover));

        let log = vec![
            ev(1, phase(Designing)),
            ev(2, phase(Approved)), // illegal: ignored
            ev(3, phase(Reviewing)),
            ev(4, phase(Agreed)),
        ];
        assert_eq!(fold_solution(ITEM, &log).phase, Agreed);
        assert_eq!(fold_solution(ITEM, &[]).phase, Requirements);
    }

    // AC-08 / TS-12 at the domain level: three rounds per cycle, a fourth is
    // refused, and only a new cycle resets the count.
    #[test]
    fn rounds_cap_at_three_per_cycle() {
        let lvl = ReviewLevel::Solution;
        let mut log = Vec::new();
        for n in 1..=3u8 {
            let next = next_round(&log, lvl, ITEM).expect("round available");
            assert_eq!(next, round(lvl, ITEM, 1, n));
            log.push(ev(n as usize, recorded(next, ReviewVerdict::Changes, &[])));
        }
        assert_eq!(rounds_used(&log, lvl, ITEM), 3);
        assert_eq!(next_round(&log, lvl, ITEM), Err(MAX_REVIEW_ROUNDS));
        // Other scopes and levels are counted separately.
        assert_eq!(rounds_used(&log, ReviewLevel::Code, ITEM), 0);
        assert_eq!(rounds_used(&log, lvl, "01OTHER"), 0);

        log.push(ev(
            9,
            InboxEventKind::ReviewCycleStarted {
                level: lvl,
                scope: ITEM.into(),
                cycle: 2,
            },
        ));
        assert_eq!(next_round(&log, lvl, ITEM), Ok(round(lvl, ITEM, 2, 1)));
        let s = fold_solution(ITEM, &log);
        assert_eq!((s.cycle, s.round, s.rounds.len()), (2, 0, 3));
    }

    // TS-54 (scope-key half): a re-created worktree has the same scope key, so
    // its rounds on the origin item carry over.
    #[test]
    fn a_recreated_worktree_keeps_its_round_count() {
        let scope = project_scope("/code/acme", "feat-x", "billing_20261111");
        assert_eq!(scope, "/code/acme@feat-x/billing_20261111");
        let lvl = ReviewLevel::Code;
        let log = vec![
            ev(
                1,
                recorded(round(lvl, &scope, 1, 1), ReviewVerdict::Changes, &[]),
            ),
            ev(
                2,
                recorded(round(lvl, &scope, 1, 2), ReviewVerdict::Changes, &[]),
            ),
        ];
        let recreated = project_scope("/code/acme", "feat-x", "billing_20261111");
        assert_eq!(rounds_used(&log, lvl, &recreated), 2);
        assert_eq!(next_round(&log, lvl, &recreated).unwrap().round, 3);
        assert_ne!(
            project_scope("/code/acme", "feat-y", "billing_20261111"),
            scope,
            "another branch is another scope"
        );
    }

    #[test]
    fn a_duplicate_round_line_is_counted_once() {
        let r = round(ReviewLevel::Solution, ITEM, 1, 1);
        let log = vec![
            ev(1, recorded(r.clone(), ReviewVerdict::Changes, &[])),
            ev(2, recorded(r, ReviewVerdict::Changes, &[])),
        ];
        assert_eq!(rounds_used(&log, ReviewLevel::Solution, ITEM), 1);
        assert_eq!(fold_solution(ITEM, &log).rounds.len(), 1);
    }

    #[test]
    fn idempotency_keys_cover_rounds_approvals_and_problems() {
        let r = round(ReviewLevel::Solution, ITEM, 1, 2);
        assert_eq!(
            idempotency_key(&recorded(r.clone(), ReviewVerdict::Agreed, &[])),
            idempotency_key(&recorded(r, ReviewVerdict::Changes, &["p"])),
            "a round is unique on (level, scope, cycle, round), whatever it says"
        );
        let approve = |h: BTreeMap<String, String>, c| InboxEventKind::DesignApproved {
            doc_hashes: h,
            credential: c,
        };
        assert_eq!(
            idempotency_key(&approve(hashes("a"), CredentialClass::OperatorSession)),
            idempotency_key(&approve(hashes("a"), CredentialClass::OperatorCli)),
        );
        assert_ne!(
            idempotency_key(&approve(hashes("a"), CredentialClass::OperatorSession)),
            idempotency_key(&approve(hashes("b"), CredentialClass::OperatorSession)),
            "a different document set is a new approval"
        );
        let handover = |h: &str| InboxEventKind::HandoverApproved {
            project: "/code/acme".into(),
            instruction_hash: h.into(),
            design_hash: "d".into(),
            branch: "feat".into(),
            base: None,
            credential: CredentialClass::OperatorSession,
        };
        assert!(idempotency_key(&handover("h1")).is_some());
        assert_ne!(
            idempotency_key(&handover("h1")),
            idempotency_key(&handover("h2"))
        );
        assert_eq!(idempotency_key(&phase(SolutionPhase::Designing)), None);
        assert_eq!(
            idempotency_key(&InboxEventKind::Comment {
                thread: crate::domain::inbox_events::Thread::Overall,
                body: "x".into(),
                warnings: vec![],
            }),
            None
        );
    }

    fn problem(n: usize, rid: &str, pid: &str) -> InboxEvent {
        ev(
            n,
            InboxEventKind::ProblemRaised {
                request_id: rid.into(),
                problem_id: pid.into(),
                round: round(ReviewLevel::Solution, ITEM, 1, 3),
                title: format!("Problem {pid}"),
                detail: "detail".into(),
                warnings: vec![],
            },
        )
    }

    // FR-18 / TS-14..16 at the domain level: one request per problem, each
    // resolved on its own through the request flow.
    #[test]
    fn each_problem_is_its_own_request_and_resolves_alone() {
        let text = "Use OIDC.";
        let log = vec![
            problem(1, "r1", "p1"),
            problem(2, "r2", "p2"),
            problem(3, "r2", "p2"), // replayed line
            ev(
                4,
                InboxEventKind::ResolutionConfirmed {
                    request_id: "r1".into(),
                    proposal_id: None,
                    content_hash: content_hash(text),
                    text: text.into(),
                    edited: false,
                    warnings: vec![],
                },
            ),
        ];
        let s = fold_solution(ITEM, &log);
        assert_eq!(s.problems.len(), 2);
        assert_eq!(s.problems[0].status, ProblemStatus::Resolved);
        assert_eq!(s.problems[1].status, ProblemStatus::Open);
        assert_eq!(s.open_problems(), 1);
    }

    // TS-53: a solution-problem request folds as a request with no worktree
    // and is never delivered to a pane.
    #[test]
    fn a_solution_problem_folds_as_a_request_without_a_pane() {
        let views: Vec<RequestView> = fold_requests(&[problem(1, "r1", "p1")]);
        assert_eq!(views.len(), 1);
        let v = &views[0];
        assert_eq!(v.origin, RequestOrigin::SolutionProblem);
        assert_eq!(v.problem_id.as_deref(), Some("p1"));
        assert_eq!(v.worktree, None);
        assert_eq!(v.body, "detail");
        assert_eq!(v.status, RequestStatus::Open);
        assert!(!v.delivers_to_pane());
    }

    // AC-10 at the domain level: approval binds to the hashes approved; any
    // edit voids it.
    #[test]
    fn a_design_approval_is_void_once_a_document_changes() {
        let log = vec![ev(
            1,
            InboxEventKind::DesignApproved {
                doc_hashes: hashes("v1"),
                credential: CredentialClass::OperatorSession,
            },
        )];
        let s = fold_solution(ITEM, &log);
        assert!(s.design_approval_valid(&hashes("v1")));
        assert!(!s.design_approval_valid(&hashes("v2")), "edited design");
        let mut added = hashes("v1");
        added.insert("extra.md".into(), sha256_hex("x"));
        assert!(
            !s.design_approval_valid(&added),
            "a new document voids it too"
        );
        assert!(!SolutionState::default().design_approval_valid(&hashes("v1")));
    }

    fn drafted(n: usize, project: &str, h: &str) -> InboxEvent {
        ev(
            n,
            InboxEventKind::HandoverDrafted {
                project: project.into(),
                instruction_hash: h.into(),
                design_hash: "d".into(),
                branch: "feat".into(),
                base: Some("main".into()),
            },
        )
    }

    fn approved(n: usize, project: &str, h: &str) -> InboxEvent {
        ev(
            n,
            InboxEventKind::HandoverApproved {
                project: project.into(),
                instruction_hash: h.into(),
                design_hash: "d".into(),
                branch: "feat".into(),
                base: Some("main".into()),
                credential: CredentialClass::OperatorSession,
            },
        )
    }

    #[test]
    fn handovers_are_approved_per_project_and_voided_by_a_redraft() {
        let log = vec![
            drafted(1, "/code/a", "h1"),
            drafted(2, "/code/b", "hb"),
            approved(3, "/code/a", "h1"),
            ev(
                4,
                InboxEventKind::HandoverDeferred {
                    project: "/code/b".into(),
                    credential: CredentialClass::OperatorSession,
                },
            ),
        ];
        let s = fold_solution(ITEM, &log);
        assert_eq!(s.handovers.len(), 2);
        assert_eq!(s.handovers[0].status, HandoverStatus::Approved);
        assert_eq!(s.handovers[1].status, HandoverStatus::Deferred);
        assert!(s.handover_approval_valid("/code/a", "h1"));
        assert!(!s.handover_approval_valid("/code/b", "hb"));

        let mut redrafted = log.clone();
        redrafted.push(drafted(5, "/code/a", "h2"));
        let s = fold_solution(ITEM, &redrafted);
        assert_eq!(s.handovers[0].status, HandoverStatus::Drafted);
        assert!(
            !s.handover_approval_valid("/code/a", "h2"),
            "new text needs a new approval"
        );

        let mut st = fold_solution(ITEM, &log);
        mark_converted(&mut st, &["/code/a".to_string(), "/code/b".to_string()]);
        assert_eq!(st.handovers[0].status, HandoverStatus::Converted);
        assert_eq!(
            st.handovers[1].status,
            HandoverStatus::Deferred,
            "deferred is never converted"
        );
    }

    #[test]
    fn the_coordinator_badge_follows_the_latest_note_write() {
        let upsert = |n, id: &str, by, h: &str| {
            ev(
                n,
                InboxEventKind::NoteUpserted {
                    note_id: id.into(),
                    by,
                    note_hash: h.into(),
                },
            )
        };
        let log = vec![
            upsert(1, "n1", NoteAuthor::Coordinator, "h1"),
            upsert(2, "n2", NoteAuthor::Coordinator, "h2"),
            upsert(3, "n2", NoteAuthor::Operator, "h3"),
            ev(
                4,
                InboxEventKind::CoordinatorTurn {
                    turn_id: "t1".into(),
                    agent: "claude".into(),
                    model: None,
                    session_id: "s".into(),
                    transcript_offset: 0,
                },
            ),
        ];
        let s = fold_solution(ITEM, &log);
        assert_eq!(
            s.coordinator_notes,
            BTreeMap::from([("n1".to_string(), "h1".to_string())])
        );
        assert_eq!(s.coordinator_turns, 1);
    }

    #[test]
    fn overrides_report_emptiness() {
        assert!(FactoryOverrides::default().is_empty());
        let o = FactoryOverrides {
            reviewer: ReviewerProfiles {
                project: Some(AgentProfile::new("codex", None)),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!o.is_empty());
        let yaml = serde_yaml::to_string(&o).unwrap();
        assert_eq!(yaml.trim(), "reviewer:\n  project:\n    agent: codex");
    }
}
