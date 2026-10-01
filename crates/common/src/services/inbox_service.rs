//! Inbox draft orchestration over [`InboxStore`] and the projects registry.
//!
//! The store owns bytes on disk; this layer owns the rules the dashboard and
//! `sebenza-cli` share: which drafts a listing shows, how a project link is
//! resolved (and what an unresolvable one looks like), and that deleting a
//! promoted draft destroys the only record of the prompts it produced.

use crate::adapters::inbox_store::{
    EventAuthor, FrontmatterAuthor, FrontmatterPatch, InboxStore, InboxStoreError, PriorityWrite,
};
use crate::adapters::projects_registry::ProjectsRegistry;
use crate::domain::inbox_events::{
    AgentSession, AuthorKind, InboxEvent, InboxEventKind, RequestStatus, RequestView, Thread,
    WorktreeKey, apply_redactions, fold_requests,
};
use crate::domain::model::{
    DraftStatus, InboxDraft, InboxDraftView, Priority, PrioritySource, ProjectRef, inbox_order,
};
use crate::services::inbox_convert::{
    ConversionOutcome, ConversionRunner, ConversionTarget, TargetError, run_conversion,
    scan_for_secrets, status_after, validate_targets,
};
use crate::services::inbox_limits::{InboxLimits, RateLimiter};
use crate::util::id::random_ulid;
use serde::Serialize;
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// Failures callers must distinguish. Everything else surfaces as the
/// underlying [`InboxStoreError`].
#[derive(Debug, Error)]
pub enum InboxServiceError {
    /// Deleting a promoted draft needs an explicit confirmation, because its
    /// `conversions[]` is the only record of the prompts that were sent.
    #[error("draft {0} is promoted; deleting it discards its conversion history")]
    ConfirmationRequired(String),
    /// The request was refused before anything ran. Carries every problem, so
    /// the caller can mark all the bad targets at once.
    #[error("{} invalid target(s)", .0.len())]
    InvalidTargets(Vec<TargetError>),
    /// A comment or request field is over its cap (T-09).
    #[error("{field} is longer than {limit}")]
    TooLarge { field: &'static str, limit: usize },
    /// The caller is over its per-window allowance (T-09).
    #[error("rate limit exceeded; try again shortly")]
    RateLimited,
    /// The item already holds as many open requests as it may (T-09).
    #[error("draft {0} has too many open requests")]
    TooManyOpenRequests(String),
    /// The claimed worktree is not one this item was converted into (T-06).
    #[error("worktree is not a conversion of draft {0}")]
    ForeignWorktree(String),
    /// A malformed comment or request.
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Store(#[from] InboxStoreError),
}

/// A draft's project link, resolved against the registry at read time. The
/// stored value is only a path, so a moved or removed project degrades to
/// [`ProjectLink::Unresolved`] rather than invalidating the draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectLink {
    Resolved { path: String, name: String },
    Unresolved { path: String },
}

/// One row of a listing.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftSummary {
    pub id: String,
    pub title: String,
    pub status: DraftStatus,
    pub updated_at: String,
    pub project: Option<ProjectLink>,
    pub priority: Priority,
    pub priority_source: PrioritySource,
    /// True when the file did not parse; it still lists, so one bad draft
    /// cannot hide the rest.
    pub is_raw: bool,
}

/// Listing filters. The default hides `Dropped`.
#[derive(Debug, Clone, Default)]
pub struct ListQuery {
    /// Case-insensitive match over title and body.
    pub search: Option<String>,
    pub include_dropped: bool,
}

/// What kind of entry a comment-thread row is. Requests, advice, proposals
/// and resolutions are all shown in their worktree's thread alongside notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentKind {
    Note,
    Request,
    Advice,
    Proposal,
    Resolution,
}

/// One row of a comment thread, with redactions already applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentView {
    pub event_id: String,
    pub ts: String,
    pub author: AuthorKind,
    /// Self-declared and unauthenticated (T-01).
    pub caller: Option<String>,
    pub kind: CommentKind,
    pub body: String,
    /// Set on a request row.
    pub title: Option<String>,
    /// Set on every row that belongs to a request.
    pub request_id: Option<String>,
    pub parent_event_id: Option<String>,
    /// Secret-scan hits. Advisory only: the body is stored unchanged (FR-10).
    pub warnings: Vec<String>,
    /// True when an operator tombstoned this row; `body` is then masked.
    pub redacted: bool,
}

/// One worktree's thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeGroup {
    pub project: String,
    pub branch: String,
    pub comments: Vec<CommentView>,
}

/// Every thread on an item: the overall one plus one per worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CommentGroups {
    pub overall: Vec<CommentView>,
    pub worktrees: Vec<WorktreeGroup>,
}

/// A metadata-only audit record (T-08, FR-34). Ids, actors and actions —
/// never a body, title or reason, which is why there is no field to hold one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditRecord {
    /// A fixed literal, e.g. `inbox.priority.changed`.
    pub action: &'static str,
    pub draft_id: String,
    pub event_id: Option<String>,
    pub request_id: Option<String>,
    /// Who the server believes acted. A worktree agent holding the control
    /// token on an operator route is recorded as `operator` (T-01).
    pub actor: AuthorKind,
    /// The unauthenticated `caller` marker the request declared.
    pub caller: Option<String>,
    pub thread: Option<&'static str>,
    pub project: Option<String>,
    pub branch: Option<String>,
    pub from: Option<Priority>,
    pub to: Option<Priority>,
    pub source: Option<PrioritySource>,
    /// Why ingress was refused, as a fixed literal (`foreign_worktree`,
    /// `rate_limited`, `too_many_open`, `too_large`).
    pub refusal: Option<&'static str>,
}

impl AuditRecord {
    fn new(action: &'static str, draft_id: &str, actor: AuthorKind) -> Self {
        Self {
            action,
            draft_id: draft_id.to_string(),
            event_id: None,
            request_id: None,
            actor,
            caller: None,
            thread: None,
            project: None,
            branch: None,
            from: None,
            to: None,
            source: None,
            refusal: None,
        }
    }

    fn with_caller(mut self, caller: &Option<String>) -> Self {
        self.caller = caller.clone();
        self
    }

    fn with_worktree(mut self, key: &WorktreeKey) -> Self {
        self.project = Some(key.project.clone());
        self.branch = Some(key.branch.clone());
        self
    }

    fn with_event(mut self, event: &InboxEvent) -> Self {
        self.event_id = Some(event.event_id.clone());
        self
    }
}

/// Where audit records go. The default writes structured `tracing` lines;
/// tests capture them instead.
pub trait AuditSink: Send + Sync {
    fn record(&self, record: &AuditRecord);
}

/// Emits each record as one `tracing` line with an `audit=` field, the same
/// shape as the existing `inbox.convert.target` line, so both grep together.
pub struct TracingAuditSink;

impl AuditSink for TracingAuditSink {
    fn record(&self, r: &AuditRecord) {
        let opt = |v: &Option<String>| v.clone().unwrap_or_default();
        let pri = |v: Option<Priority>| v.map(|p| format!("{p:?}")).unwrap_or_default();
        tracing::info!(
            audit = r.action,
            draft_id = %r.draft_id,
            event_id = %opt(&r.event_id),
            request_id = %opt(&r.request_id),
            actor = ?r.actor,
            caller = r.caller.as_deref().unwrap_or("none"),
            thread = r.thread.unwrap_or(""),
            project = %opt(&r.project),
            branch = %opt(&r.branch),
            from = %pri(r.from),
            to = %pri(r.to),
            source = %r.source.map(|s| format!("{s:?}")).unwrap_or_default(),
            refusal = r.refusal.unwrap_or(""),
            "inbox audit"
        );
    }
}

/// Told when a request opens. Phase 3's `SystemAgentService` implements this
/// to enqueue triage; until then nothing is registered and a request simply
/// waits, open, for the operator.
pub trait RequestObserver: Send + Sync {
    fn request_opened(&self, draft_id: &str, request_id: &str);
}

/// What a worktree agent sent through `sebenza-agentctl request|comment`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngressKind {
    Request { title: Option<String>, body: String },
    Comment { body: String },
}

/// A worktree's inbox ingress, as claimed. Nothing here is authenticated
/// beyond the shared control token: the draft id comes from the worktree's
/// own `inbox-origin.json` and the path from its environment, which is why
/// [`InboxService::ingest`] cross-checks both against `conversions[]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeIngress {
    pub draft_id: String,
    pub worktree_path: String,
    pub branch: String,
    pub kind: IngressKind,
}

/// Runtime-event `type`s that carry inbox ingress.
pub const INGRESS_REQUEST_TYPE: &str = "inbox.request";
pub const INGRESS_COMMENT_TYPE: &str = "inbox.comment";

/// Parse a `/api/runtime/events` body as inbox ingress. `None` when the
/// `type` is not an inbox one (the caller handles it as a runtime event);
/// `Some(Err)` when it is, but malformed.
pub fn parse_worktree_ingress(raw: &serde_json::Value) -> Option<Result<WorktreeIngress, String>> {
    let kind = raw.get("type")?.as_str()?;
    if !kind.starts_with("inbox.") {
        return None;
    }
    let field = |key: &str| {
        raw.get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .map(str::to_string)
    };
    let required = |key: &'static str| field(key).ok_or_else(|| format!("{key} is required"));
    Some((|| {
        let draft_id = required("draftId")?;
        let worktree_path = required("worktreePath")?;
        let branch = required("branch")?;
        let body = required("body")?;
        // The error never echoes `kind`: it is caller-chosen text.
        let kind = match kind {
            INGRESS_REQUEST_TYPE => IngressKind::Request {
                title: field("title"),
                body,
            },
            INGRESS_COMMENT_TYPE => IngressKind::Comment { body },
            _ => return Err("unknown inbox event type".to_string()),
        };
        Ok(WorktreeIngress {
            draft_id,
            worktree_path,
            branch,
            kind,
        })
    })())
}

/// Reads and writes drafts under the rules above.
pub struct InboxService {
    store: InboxStore,
    projects: ProjectsRegistry,
    limits: InboxLimits,
    comment_rate: RateLimiter,
    request_rate: RateLimiter,
    audit: Arc<dyn AuditSink>,
    observer: RwLock<Option<Arc<dyn RequestObserver>>>,
}

impl InboxService {
    /// Build over an explicit store and registry, with default limits and
    /// `tracing` audit.
    pub fn new(store: InboxStore, projects: ProjectsRegistry) -> Self {
        let limits = InboxLimits::default();
        Self {
            store,
            projects,
            limits,
            comment_rate: RateLimiter::new(limits.comments),
            request_rate: RateLimiter::new(limits.requests),
            audit: Arc::new(TracingAuditSink),
            observer: RwLock::new(None),
        }
    }

    /// Replace the ingress limits.
    pub fn with_limits(mut self, limits: InboxLimits) -> Self {
        self.limits = limits;
        self.comment_rate = RateLimiter::new(limits.comments);
        self.request_rate = RateLimiter::new(limits.requests);
        self
    }

    /// Send audit records somewhere other than `tracing`.
    pub fn with_audit_sink(mut self, sink: Arc<dyn AuditSink>) -> Self {
        self.audit = sink;
        self
    }

    /// Register the hook told about every newly opened request.
    pub fn set_request_observer(&self, observer: Arc<dyn RequestObserver>) {
        *self.observer.write().unwrap_or_else(|e| e.into_inner()) = Some(observer);
    }

    /// Resolve a stored project path against the registry. Matching is by exact
    /// path, the key the registry itself is indexed on.
    fn resolve_project(&self, project: Option<&ProjectRef>) -> Option<ProjectLink> {
        let path = project?.path.clone();
        match self.projects.list().into_iter().find(|p| p.path == path) {
            Some(entry) => Some(ProjectLink::Resolved {
                path,
                name: entry.name,
            }),
            None => Some(ProjectLink::Unresolved { path }),
        }
    }

    /// Drafts by priority, then newest first (`inbox_order`), `Dropped`
    /// hidden unless asked for, filtered by `search` over title and body.
    pub fn list(&self, query: &ListQuery) -> Result<Vec<DraftSummary>, InboxServiceError> {
        let needle = query.search.as_ref().map(|s| s.to_lowercase());
        let mut views = self.store.list()?;
        views.sort_by(inbox_order);
        let mut out = Vec::new();
        for view in views {
            let summary = match &view {
                InboxDraftView::Parsed(draft) => {
                    if draft.frontmatter.status == DraftStatus::Dropped && !query.include_dropped {
                        continue;
                    }
                    if let Some(needle) = &needle {
                        let hit = draft.frontmatter.title.to_lowercase().contains(needle)
                            || draft.body.to_lowercase().contains(needle);
                        if !hit {
                            continue;
                        }
                    }
                    DraftSummary {
                        id: draft.id.clone(),
                        title: draft.frontmatter.title.clone(),
                        status: draft.frontmatter.status,
                        updated_at: draft.frontmatter.updated_at.clone(),
                        project: self.resolve_project(draft.frontmatter.project.as_ref()),
                        priority: draft.frontmatter.priority,
                        priority_source: draft.frontmatter.priority_source,
                        is_raw: false,
                    }
                }
                // A draft we cannot parse has no title, status or project to
                // filter on. It lists regardless so it stays reachable — and
                // therefore fixable — rather than vanishing from the UI.
                InboxDraftView::Raw { id, raw_text, .. } => {
                    if let Some(needle) = &needle
                        && !raw_text.to_lowercase().contains(needle)
                    {
                        continue;
                    }
                    DraftSummary {
                        id: id.clone(),
                        title: String::new(),
                        status: DraftStatus::Draft,
                        updated_at: String::new(),
                        project: None,
                        priority: Priority::default(),
                        priority_source: PrioritySource::default(),
                        is_raw: true,
                    }
                }
            };
            out.push(summary);
        }
        Ok(out)
    }

    /// One draft, with its project link resolved.
    pub fn get(
        &self,
        id: &str,
    ) -> Result<(InboxDraftView, Option<ProjectLink>), InboxServiceError> {
        let view = self.store.get(id)?;
        let link = match &view {
            InboxDraftView::Parsed(draft) => {
                self.resolve_project(draft.frontmatter.project.as_ref())
            }
            InboxDraftView::Raw { .. } => None,
        };
        Ok((view, link))
    }

    /// Create an empty draft.
    pub fn create(&self, title: &str) -> Result<InboxDraft, InboxServiceError> {
        Ok(self.store.create(title)?)
    }

    /// Replace the body, gated on the body hash the caller last read.
    pub fn save_body(
        &self,
        id: &str,
        expected_hash: &str,
        body: &str,
    ) -> Result<InboxDraft, InboxServiceError> {
        Ok(self.store.save_body(id, expected_hash, body)?)
    }

    /// Rename. The title never reaches the filename.
    pub fn rename(&self, id: &str, title: &str) -> Result<InboxDraft, InboxServiceError> {
        self.edit(
            id,
            FrontmatterPatch {
                title: Some(title.to_string()),
                ..Default::default()
            },
        )
    }

    /// Link the draft to a project by absolute path. The path is not validated
    /// against the registry — an unknown one simply reads back unresolved.
    pub fn link_project(&self, id: &str, path: &str) -> Result<InboxDraft, InboxServiceError> {
        self.edit(
            id,
            FrontmatterPatch {
                project: Some(Some(ProjectRef {
                    path: path.to_string(),
                })),
                ..Default::default()
            },
        )
    }

    /// Remove the project link.
    pub fn unlink_project(&self, id: &str) -> Result<InboxDraft, InboxServiceError> {
        self.edit(
            id,
            FrontmatterPatch {
                project: Some(None),
                ..Default::default()
            },
        )
    }

    /// Move the draft to `Dropped`, the terminal state for an idea that was
    /// considered and not taken.
    pub fn drop_draft(&self, id: &str) -> Result<InboxDraft, InboxServiceError> {
        self.edit(
            id,
            FrontmatterPatch {
                status: Some(DraftStatus::Dropped),
                ..Default::default()
            },
        )
    }

    /// Delete the file. A `Promoted` draft requires `confirmed`, otherwise this
    /// returns [`InboxServiceError::ConfirmationRequired`] and writes nothing.
    pub fn delete(&self, id: &str, confirmed: bool) -> Result<(), InboxServiceError> {
        if !confirmed
            && let InboxDraftView::Parsed(draft) = self.store.get(id)?
            && draft.frontmatter.status == DraftStatus::Promoted
        {
            return Err(InboxServiceError::ConfirmationRequired(id.to_string()));
        }
        Ok(self.store.delete(id)?)
    }

    /// Convert a draft into worktrees.
    ///
    /// Validates the whole request first and runs nothing if any target is
    /// bad. Then each target's outcome is merged into `conversions[]` as it
    /// completes, so a crash midway leaves a draft that still knows what
    /// happened. `Promoted` is set at the end, and only if something was
    /// actually created.
    pub fn convert<R: ConversionRunner>(
        &self,
        id: &str,
        targets: &[ConversionTarget],
        runner: &R,
    ) -> Result<Vec<ConversionOutcome>, InboxServiceError> {
        self.convert_streaming(id, targets, runner, |_| {})
    }

    /// As [`Self::convert`], but `observe` is called with each outcome as it is
    /// persisted — how a caller reports progress without waiting for the wave.
    pub fn convert_streaming<R: ConversionRunner, F: FnMut(&ConversionOutcome)>(
        &self,
        id: &str,
        targets: &[ConversionTarget],
        runner: &R,
        mut observe: F,
    ) -> Result<Vec<ConversionOutcome>, InboxServiceError> {
        let draft = match self.store.get(id)? {
            InboxDraftView::Parsed(d) => d,
            InboxDraftView::Raw { id, error, .. } => {
                return Err(InboxServiceError::Store(InboxStoreError::Unparsed {
                    id,
                    error,
                }));
            }
        };

        let known: Vec<String> = self.projects.list().into_iter().map(|p| p.path).collect();
        let errors = validate_targets(targets, &known);
        if !errors.is_empty() {
            return Err(InboxServiceError::InvalidTargets(errors));
        }

        // Prior waves are kept: the draft is the ledger of everything it has
        // ever produced, not just the most recent run.
        let mut recorded = draft.frontmatter.conversions.clone();
        let outcomes = run_conversion(runner, id, &draft.body, targets, |outcome| {
            if let Ok(value) = serde_yaml::to_value(outcome) {
                recorded.push(value);
                // Flush immediately; losing a back-link to a worktree that
                // exists is worse than an extra write.
                let _ = self.store.merge_frontmatter(
                    id,
                    FrontmatterAuthor::Job,
                    FrontmatterPatch {
                        conversions: Some(recorded.clone()),
                        ..Default::default()
                    },
                );
            }
            observe(outcome);
        });

        if status_after(draft.frontmatter.status, &outcomes) == DraftStatus::Promoted {
            let _ = self.store.merge_frontmatter(
                id,
                FrontmatterAuthor::Job,
                FrontmatterPatch {
                    status: Some(DraftStatus::Promoted),
                    ..Default::default()
                },
            );
        }
        Ok(outcomes)
    }

    /// A draft's conversion history, as recorded in its own frontmatter.
    ///
    /// This is the durable record. The in-memory job manager is a cache that
    /// dies with the process; after a restart this is the only thing that
    /// still knows what a wave produced, which is why each outcome is flushed
    /// as it happens rather than at the end.
    pub fn conversion_history(
        &self,
        id: &str,
    ) -> Result<Vec<ConversionOutcome>, InboxServiceError> {
        let draft = match self.store.get(id)? {
            InboxDraftView::Parsed(d) => d,
            InboxDraftView::Raw { id, error, .. } => {
                return Err(InboxServiceError::Store(InboxStoreError::Unparsed {
                    id,
                    error,
                }));
            }
        };
        Ok(draft
            .frontmatter
            .conversions
            .into_iter()
            // A hand-edited or older entry that no longer deserializes is
            // skipped rather than failing the read: partial history beats none.
            .filter_map(|v| serde_yaml::from_value(v).ok())
            .collect())
    }

    /// Every editor-authored frontmatter change goes through here, so the
    /// author is stated once and the store's ownership rules do the rest.
    fn edit(&self, id: &str, patch: FrontmatterPatch) -> Result<InboxDraft, InboxServiceError> {
        Ok(self
            .store
            .merge_frontmatter(id, FrontmatterAuthor::Editor, patch)?)
    }
}

// --- Priority, comments and requests ----------------------------------------

impl InboxService {
    /// Operator sets (`Some`) or clears (`None`) the priority override.
    /// Clearing keeps the value and hands control back to the agent.
    /// `caller` is the unauthenticated marker the request declared.
    pub fn set_priority(
        &self,
        id: &str,
        priority: Option<Priority>,
        caller: Option<String>,
    ) -> Result<InboxDraft, InboxServiceError> {
        let before = self.parsed(id)?.frontmatter;
        let event =
            self.store
                .set_priority_by(id, PriorityWrite::Operator(priority), caller.clone())?;
        let after = self.parsed(id)?;
        let fm = &after.frontmatter;
        let action = match (&event, fm.priority_source) {
            (Some(_), _) => Some("inbox.priority.changed"),
            // Same value, new owner: no `priority_changed` event, but the
            // operator still made a decision worth recording.
            _ if before.priority_source == fm.priority_source => None,
            (None, PrioritySource::Operator) => Some("inbox.priority.override_set"),
            (None, PrioritySource::Agent) => Some("inbox.priority.override_cleared"),
        };
        if let Some(action) = action {
            let mut record =
                AuditRecord::new(action, id, AuthorKind::Operator).with_caller(&caller);
            record.from = Some(before.priority);
            record.to = Some(fm.priority);
            record.source = Some(fm.priority_source);
            if let Some(e) = &event {
                record = record.with_event(e);
            }
            self.audit.record(&record);
        }
        Ok(after)
    }

    /// The system agent's priority write. A no-op while an operator override
    /// stands (BR-02); returns the `priority_changed` event when it moved.
    pub fn set_agent_priority(
        &self,
        id: &str,
        priority: Priority,
    ) -> Result<Option<InboxEvent>, InboxServiceError> {
        let event = self
            .store
            .set_priority(id, PriorityWrite::Agent(priority))?;
        if let Some(e) = &event
            && let InboxEventKind::PriorityChanged { from, to, source } = &e.kind
        {
            let mut record =
                AuditRecord::new("inbox.priority.changed", id, AuthorKind::SystemAgent)
                    .with_event(e);
            record.from = Some(*from);
            record.to = Some(*to);
            record.source = Some(*source);
            self.audit.record(&record);
        }
        Ok(event)
    }

    /// The item's system-agent session, if it has one.
    pub fn read_session(&self, id: &str) -> Result<Option<AgentSession>, InboxServiceError> {
        Ok(self.store.read_session(id)?)
    }

    /// Persist the item's system-agent session. Server-only: no route writes it.
    pub fn write_session(&self, id: &str, session: &AgentSession) -> Result<(), InboxServiceError> {
        Ok(self.store.write_session(id, session)?)
    }

    /// The item's event log with redactions applied.
    pub fn events(&self, id: &str) -> Result<Vec<InboxEvent>, InboxServiceError> {
        self.parsed(id)?;
        let mut events = self.store.read_events(id)?;
        apply_redactions(&mut events);
        Ok(events)
    }

    /// Every thread: overall, then one group per converted worktree (in
    /// conversion order) plus any worktree that only appears in the log.
    pub fn list_comments(&self, id: &str) -> Result<CommentGroups, InboxServiceError> {
        let draft = self.parsed(id)?;
        let events = self.events(id)?;
        let redacted = redacted_ids(&events);
        let requests = fold_requests(&events);

        let mut groups = CommentGroups::default();
        for key in worktree_keys(&draft) {
            group_for(&mut groups, &key);
        }
        for event in &events {
            let Some(row) = comment_row(event, &redacted) else {
                continue;
            };
            let thread = match (&event.kind, &row.request_id) {
                (InboxEventKind::Comment { thread, .. }, _) => thread.clone(),
                (_, Some(rid)) => requests
                    .iter()
                    .find(|r| &r.request_id == rid)
                    .map(|r| Thread::Worktree(r.worktree.clone()))
                    .unwrap_or(Thread::Overall),
                (_, None) => Thread::Overall,
            };
            match thread {
                Thread::Overall => groups.overall.push(row),
                Thread::Worktree(key) => group_for(&mut groups, &key).comments.push(row),
            }
        }
        Ok(groups)
    }

    /// The thread row for one event, or `None` for events that are not rows
    /// (priority changes, tombstones, delivery bookkeeping).
    pub fn comment_view(&self, event: &InboxEvent) -> Option<CommentView> {
        comment_row(event, &std::collections::HashSet::new())
    }

    /// Append a comment. A worktree thread must name a worktree this item
    /// was converted into.
    pub fn add_comment(
        &self,
        id: &str,
        author: EventAuthor,
        thread: Thread,
        body: &str,
    ) -> Result<InboxEvent, InboxServiceError> {
        if body.trim().is_empty() {
            return Err(InboxServiceError::Invalid(
                "a comment needs a body".to_string(),
            ));
        }
        self.cap("body", body.len(), self.limits.max_body_bytes)?;
        let draft = self.parsed(id)?;
        if let Thread::Worktree(key) = &thread
            && !worktree_keys(&draft).contains(key)
        {
            return Err(InboxServiceError::Invalid(format!(
                "{}:{} is not a worktree this item was converted into",
                key.project, key.branch
            )));
        }
        let worktree = match &thread {
            Thread::Worktree(key) => Some(key),
            Thread::Overall => None,
        };
        if let Some(key) = rate_key(&author, worktree)
            && !self.comment_rate.allow(&key)
        {
            return Err(InboxServiceError::RateLimited);
        }
        let mut record =
            AuditRecord::new("inbox.comment.added", id, author.kind).with_caller(&author.caller);
        record.thread = Some(if worktree.is_some() {
            "worktree"
        } else {
            "overall"
        });
        if let Some(key) = worktree {
            record = record.with_worktree(key);
        }
        let event = self.store.append_event(
            id,
            author,
            None,
            InboxEventKind::Comment {
                thread,
                body: body.to_string(),
                warnings: secret_warnings(body),
            },
        )?;
        self.audit.record(&record.with_event(&event));
        Ok(event)
    }

    /// Open a request in `worktree`'s group and tell the observer.
    pub fn open_request(
        &self,
        id: &str,
        author: EventAuthor,
        worktree: WorktreeKey,
        title: &str,
        body: &str,
    ) -> Result<InboxEvent, InboxServiceError> {
        let title = title.trim();
        if title.is_empty() || body.trim().is_empty() {
            return Err(InboxServiceError::Invalid(
                "a request needs a title and a body".to_string(),
            ));
        }
        self.cap("title", title.chars().count(), self.limits.max_title_chars)?;
        self.cap("body", body.len(), self.limits.max_body_bytes)?;
        let draft = self.parsed(id)?;
        if !worktree_keys(&draft).contains(&worktree) {
            return Err(InboxServiceError::ForeignWorktree(id.to_string()));
        }
        // Depth bound: each open request will cost a triage turn, so an item
        // stops accepting new ones until some are dealt with (TA-R1).
        let open = fold_requests(&self.store.read_events(id)?)
            .iter()
            .filter(|r| r.status != RequestStatus::Resolved)
            .count();
        if open >= self.limits.max_open_requests {
            return Err(InboxServiceError::TooManyOpenRequests(id.to_string()));
        }
        if let Some(key) = rate_key(&author, Some(&worktree))
            && !self.request_rate.allow(&key)
        {
            return Err(InboxServiceError::RateLimited);
        }
        let mut record = AuditRecord::new("inbox.request.opened", id, author.kind)
            .with_caller(&author.caller)
            .with_worktree(&worktree);
        record.thread = Some("worktree");
        let request_id = random_ulid();
        let event = self.store.append_event(
            id,
            author,
            None,
            InboxEventKind::RequestOpened {
                request_id: request_id.clone(),
                worktree,
                title: title.to_string(),
                body: body.to_string(),
                warnings: secret_warnings(&format!("{title}\n{body}")),
            },
        )?;
        record.request_id = Some(request_id.clone());
        self.audit.record(&record.with_event(&event));
        // Phase 3 hangs triage here. With no observer the request simply
        // waits, open, for the operator.
        let observer = self
            .observer
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(observer) = observer {
            observer.request_opened(id, &request_id);
        }
        Ok(event)
    }

    /// Every request on the item, folded from the log, redactions applied.
    pub fn list_requests(&self, id: &str) -> Result<Vec<RequestView>, InboxServiceError> {
        Ok(fold_requests(&self.events(id)?))
    }

    /// Apply a worktree agent's request or comment, after checking the
    /// claimed worktree is one this item was converted into (T-06, T-37).
    pub fn ingest(&self, ingress: &WorktreeIngress) -> Result<InboxEvent, InboxServiceError> {
        let result = self.ingest_unaudited(ingress);
        if let Err(err) = &result {
            let refusal = match err {
                InboxServiceError::ForeignWorktree(_) => Some("foreign_worktree"),
                InboxServiceError::RateLimited => Some("rate_limited"),
                InboxServiceError::TooManyOpenRequests(_) => Some("too_many_open"),
                InboxServiceError::TooLarge { .. } => Some("too_large"),
                _ => None,
            };
            if let Some(refusal) = refusal {
                let author = EventAuthor::worktree_agent();
                let mut record =
                    AuditRecord::new("inbox.ingress.refused", &ingress.draft_id, author.kind)
                        .with_caller(&author.caller);
                // The claimed branch, not the path: the record is metadata
                // about the refusal, not a copy of what was sent.
                record.branch = Some(ingress.branch.clone());
                record.refusal = Some(refusal);
                self.audit.record(&record);
            }
        }
        result
    }

    fn ingest_unaudited(&self, ingress: &WorktreeIngress) -> Result<InboxEvent, InboxServiceError> {
        let draft = self.parsed(&ingress.draft_id)?;
        let claimed = ingress.worktree_path.trim_end_matches('/');
        // Both halves must match one created conversion: the path from the
        // worktree's environment and the branch from its control.env. The
        // draft id alone is just a file the worktree could have rewritten.
        let conversion = created_conversions(&draft)
            .into_iter()
            .find(|c| {
                c.branch == ingress.branch
                    && c.worktree_path
                        .as_deref()
                        .is_some_and(|p| p.trim_end_matches('/') == claimed)
            })
            .ok_or_else(|| InboxServiceError::ForeignWorktree(draft.id.clone()))?;
        let key = WorktreeKey {
            project: conversion.project_path,
            branch: conversion.branch,
        };
        let author = EventAuthor::worktree_agent();
        match &ingress.kind {
            IngressKind::Request { title, body } => {
                let title = title
                    .as_deref()
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| title_from(body));
                self.open_request(&draft.id, author, key, &title, body)
            }
            IngressKind::Comment { body } => {
                self.add_comment(&draft.id, author, Thread::Worktree(key), body)
            }
        }
    }

    fn cap(&self, field: &'static str, len: usize, limit: usize) -> Result<(), InboxServiceError> {
        if len > limit {
            return Err(InboxServiceError::TooLarge { field, limit });
        }
        Ok(())
    }

    /// The draft, or an error if it is missing or does not parse.
    fn parsed(&self, id: &str) -> Result<InboxDraft, InboxServiceError> {
        match self.store.get(id)? {
            InboxDraftView::Parsed(d) => Ok(d),
            InboxDraftView::Raw { id, error, .. } => {
                Err(InboxServiceError::Store(InboxStoreError::Unparsed {
                    id,
                    error,
                }))
            }
        }
    }
}

/// Who a rate limit counts against. A worktree agent is limited per worktree
/// (the only identity it has); an operator-route caller per declared marker.
/// The system agent's writes are server-driven and not limited here.
fn rate_key(author: &EventAuthor, worktree: Option<&WorktreeKey>) -> Option<String> {
    match author.kind {
        AuthorKind::SystemAgent => None,
        AuthorKind::WorktreeAgent => Some(match worktree {
            Some(k) => format!("worktree:{}:{}", k.project, k.branch),
            None => "worktree:-".to_string(),
        }),
        AuthorKind::Operator => Some(format!(
            "operator:{}",
            author.caller.as_deref().unwrap_or("-")
        )),
    }
}

/// Every worktree this draft was successfully converted into, in order.
fn worktree_keys(draft: &InboxDraft) -> Vec<WorktreeKey> {
    let mut out: Vec<WorktreeKey> = Vec::new();
    for outcome in created_conversions(draft) {
        let key = WorktreeKey {
            project: outcome.project_path,
            branch: outcome.branch,
        };
        if !out.contains(&key) {
            out.push(key);
        }
    }
    out
}

/// The created entries of `conversions[]`. An entry that no longer parses
/// is skipped, as [`InboxService::conversion_history`] does.
fn created_conversions(draft: &InboxDraft) -> Vec<ConversionOutcome> {
    draft
        .frontmatter
        .conversions
        .iter()
        .filter_map(|v| serde_yaml::from_value::<ConversionOutcome>(v.clone()).ok())
        .filter(ConversionOutcome::is_created)
        .collect()
}

fn group_for<'a>(groups: &'a mut CommentGroups, key: &WorktreeKey) -> &'a mut WorktreeGroup {
    let at = match groups
        .worktrees
        .iter()
        .position(|g| g.project == key.project && g.branch == key.branch)
    {
        Some(i) => i,
        None => {
            groups.worktrees.push(WorktreeGroup {
                project: key.project.clone(),
                branch: key.branch.clone(),
                comments: Vec::new(),
            });
            groups.worktrees.len() - 1
        }
    };
    &mut groups.worktrees[at]
}

fn redacted_ids(events: &[InboxEvent]) -> std::collections::HashSet<String> {
    events
        .iter()
        .filter_map(|e| match &e.kind {
            InboxEventKind::Redacted { target_event_id } => Some(target_event_id.clone()),
            _ => None,
        })
        .collect()
}

fn comment_row(
    event: &InboxEvent,
    redacted: &std::collections::HashSet<String>,
) -> Option<CommentView> {
    let (kind, body, title, request_id, warnings) = match &event.kind {
        InboxEventKind::Comment { body, warnings, .. } => {
            (CommentKind::Note, body, None, None, warnings.clone())
        }
        InboxEventKind::RequestOpened {
            request_id,
            title,
            body,
            warnings,
            ..
        } => (
            CommentKind::Request,
            body,
            Some(title.clone()),
            Some(request_id),
            warnings.clone(),
        ),
        InboxEventKind::Advice { request_id, body } => (
            CommentKind::Advice,
            body,
            None,
            Some(request_id),
            Vec::new(),
        ),
        InboxEventKind::Proposal {
            request_id, body, ..
        } => (
            CommentKind::Proposal,
            body,
            None,
            Some(request_id),
            Vec::new(),
        ),
        InboxEventKind::ResolutionConfirmed {
            request_id, text, ..
        } => (
            CommentKind::Resolution,
            text,
            None,
            Some(request_id),
            Vec::new(),
        ),
        _ => return None,
    };
    Some(CommentView {
        event_id: event.event_id.clone(),
        ts: event.ts.clone(),
        author: event.author,
        caller: event.caller.clone(),
        kind,
        body: body.clone(),
        title,
        request_id: request_id.cloned(),
        parent_event_id: event.parent_event_id.clone(),
        warnings,
        redacted: redacted.contains(&event.event_id),
    })
}

/// A request title from the first non-blank line of its body, cut to 80
/// characters on a char boundary.
fn title_from(body: &str) -> String {
    let line = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    line.chars().take(80).collect()
}

/// Secret-scan hits for a comment or request body: warnings only (FR-10).
fn secret_warnings(text: &str) -> Vec<String> {
    scan_for_secrets(text)
        .into_iter()
        .map(|a| a.message)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::projects_registry::ProjectEntry;
    use crate::domain::model::FileRevision;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    fn service() -> InboxService {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("sebenza-inbox-svc-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("temp base");
        let store = InboxStore::with_dir(base.join("inbox"));
        let projects = ProjectsRegistry::with_file(base.join("projects.json"));
        InboxService::new(store, projects)
    }

    fn body_hash(s: &InboxService, id: &str) -> String {
        let (view, _) = s.get(id).expect("get");
        match view {
            InboxDraftView::Parsed(d) => FileRevision::of_body(&d.body).body_hash,
            InboxDraftView::Raw { .. } => panic!("expected a parsed draft"),
        }
    }

    #[test]
    fn create_then_list_returns_the_draft() {
        let s = service();
        let draft = s.create("Inbox notes").expect("create");
        let listed = s.list(&ListQuery::default()).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, draft.id);
        assert_eq!(listed[0].title, "Inbox notes");
        assert_eq!(listed[0].status, DraftStatus::Draft);
        assert!(listed[0].project.is_none());
        assert!(!listed[0].is_raw);
    }

    #[test]
    fn list_hides_dropped_until_asked() {
        let s = service();
        let keep = s.create("Keep").expect("create");
        let gone = s.create("Drop me").expect("create");
        s.drop_draft(&gone.id).expect("drop");

        let default = s.list(&ListQuery::default()).expect("list");
        let ids: Vec<_> = default.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![keep.id.as_str()],
            "Dropped must not list by default"
        );

        let all = s
            .list(&ListQuery {
                include_dropped: true,
                ..Default::default()
            })
            .expect("list all");
        assert_eq!(all.len(), 2);
        let dropped = all.iter().find(|d| d.id == gone.id).expect("dropped row");
        assert_eq!(dropped.status, DraftStatus::Dropped);
    }

    #[test]
    fn search_matches_title_and_body_case_insensitively() {
        let s = service();
        let by_title = s.create("Postgres migration").expect("create");
        let by_body = s.create("Unrelated").expect("create");
        let h = body_hash(&s, &by_body.id);
        s.save_body(&by_body.id, &h, "we should look at POSTGRES pooling")
            .expect("save body");
        s.create("Nothing to see").expect("create");

        let hits = s
            .list(&ListQuery {
                search: Some("postgres".into()),
                ..Default::default()
            })
            .expect("search");
        let mut ids: Vec<_> = hits.iter().map(|d| d.id.clone()).collect();
        ids.sort();
        let mut want = vec![by_title.id, by_body.id];
        want.sort();
        assert_eq!(ids, want, "search must cover title and body, ignoring case");
    }

    #[test]
    fn project_link_resolves_through_the_registry() {
        let s = service();
        s.projects.add(ProjectEntry {
            path: "/home/dev/acme".into(),
            name: "acme".into(),
            added_at: 0,
        });
        let draft = s.create("Linked").expect("create");
        s.link_project(&draft.id, "/home/dev/acme").expect("link");

        let (_, link) = s.get(&draft.id).expect("get");
        assert_eq!(
            link,
            Some(ProjectLink::Resolved {
                path: "/home/dev/acme".into(),
                name: "acme".into(),
            })
        );
    }

    #[test]
    fn unknown_project_path_reads_back_unresolved() {
        let s = service();
        let draft = s.create("Moved repo").expect("create");
        s.link_project(&draft.id, "/home/dev/moved-away")
            .expect("link");

        let (_, link) = s.get(&draft.id).expect("get");
        assert_eq!(
            link,
            Some(ProjectLink::Unresolved {
                path: "/home/dev/moved-away".into(),
            }),
            "a broken link must degrade, never block the draft"
        );

        let listed = s.list(&ListQuery::default()).expect("list");
        assert_eq!(listed[0].project, link, "listing resolves the same way");
    }

    #[test]
    fn unlink_clears_the_project() {
        let s = service();
        let draft = s.create("Unlink me").expect("create");
        s.link_project(&draft.id, "/home/dev/acme").expect("link");
        s.unlink_project(&draft.id).expect("unlink");

        let (_, link) = s.get(&draft.id).expect("get");
        assert_eq!(link, None);
    }

    #[test]
    fn rename_changes_the_title_and_not_the_id() {
        let s = service();
        let draft = s.create("Before").expect("create");
        let renamed = s.rename(&draft.id, "After").expect("rename");

        assert_eq!(
            renamed.id, draft.id,
            "the id is the filename; it must not move"
        );
        assert_eq!(renamed.frontmatter.title, "After");
        let listed = s.list(&ListQuery::default()).expect("list");
        assert_eq!(listed[0].title, "After");
    }

    #[test]
    fn deleting_a_promoted_draft_requires_confirmation() {
        let s = service();
        let draft = s.create("Shipped").expect("create");
        s.store
            .merge_frontmatter(
                &draft.id,
                FrontmatterAuthor::Job,
                FrontmatterPatch {
                    status: Some(DraftStatus::Promoted),
                    ..Default::default()
                },
            )
            .expect("promote");

        let err = s.delete(&draft.id, false).expect_err("must refuse");
        assert!(
            matches!(err, InboxServiceError::ConfirmationRequired(ref id) if id == &draft.id),
            "got {err:?}"
        );
        assert!(
            s.get(&draft.id).is_ok(),
            "a refused delete must leave the draft on disk"
        );

        s.delete(&draft.id, true).expect("confirmed delete");
        assert!(s.get(&draft.id).is_err(), "confirmed delete removes it");
    }

    #[test]
    fn deleting_an_unpromoted_draft_needs_no_confirmation() {
        let s = service();
        let draft = s.create("Just a note").expect("create");
        s.delete(&draft.id, false).expect("delete");
        assert!(s.list(&ListQuery::default()).expect("list").is_empty());
    }

    #[test]
    fn a_malformed_draft_still_lists() {
        let s = service();
        let good = s.create("Good").expect("create");
        std::fs::write(
            s.store.dir().join("01ARZ3NDEKTSV4RRFFQ69G5FAV.md"),
            "---\nstatus: [unterminated\n---\nbody\n",
        )
        .expect("write malformed");

        let listed = s.list(&ListQuery::default()).expect("list");
        assert_eq!(listed.len(), 2, "one bad draft must not hide the rest");
        assert!(listed.iter().any(|d| d.id == good.id && !d.is_raw));
        assert!(listed.iter().any(|d| d.is_raw));
    }

    // --- convert ----------------------------------------------------------

    use crate::services::inbox_convert::{ConversionTarget, MAX_TARGETS};
    use std::cell::RefCell;

    struct StubRunner {
        fail: Vec<String>,
        created: RefCell<Vec<String>>,
    }

    impl StubRunner {
        fn new(fail: &[&str]) -> Self {
            Self {
                fail: fail.iter().map(|s| s.to_string()).collect(),
                created: RefCell::new(Vec::new()),
            }
        }
    }

    impl ConversionRunner for StubRunner {
        fn create_worktree(&self, t: &ConversionTarget) -> Result<String, String> {
            if self.fail.contains(&t.branch) {
                return Err("nope".into());
            }
            self.created.borrow_mut().push(t.branch.clone());
            Ok(format!("/wt/{}", t.branch))
        }
        fn write_note(&self, _p: &str, _b: &str) -> Result<(), String> {
            Ok(())
        }
        fn exclude_note(&self, _p: &str) -> Result<(), String> {
            Ok(())
        }
        fn record_origin(&self, _p: &str, _d: &str) -> Result<(), String> {
            Ok(())
        }
        fn send_prompt(&self, _t: &ConversionTarget, _p: &str) -> Result<(), String> {
            Ok(())
        }
        fn now(&self) -> String {
            "2026-09-28T00:00:00Z".into()
        }
    }

    fn convertible(s: &InboxService, branch: &str) -> (String, ConversionTarget) {
        s.projects.add(ProjectEntry {
            path: "/code/acme".into(),
            name: "acme".into(),
            added_at: 0,
        });
        let draft = s.create("Idea").expect("create");
        (
            draft.id,
            ConversionTarget {
                project_path: "/code/acme".into(),
                branch: branch.into(),
                base_branch: None,
                agent_id: Some("claude".into()),
                prompt: "build it".into(),
            },
        )
    }

    fn status_of(s: &InboxService, id: &str) -> DraftStatus {
        match s.get(id).expect("get").0 {
            InboxDraftView::Parsed(d) => d.frontmatter.status,
            _ => panic!("unparsed"),
        }
    }

    fn conversions_of(s: &InboxService, id: &str) -> Vec<serde_yaml::Value> {
        match s.get(id).expect("get").0 {
            InboxDraftView::Parsed(d) => d.frontmatter.conversions,
            _ => panic!("unparsed"),
        }
    }

    #[test]
    fn a_successful_conversion_promotes_and_records() {
        let s = service();
        let (id, target) = convertible(&s, "feature-x");
        let outcomes = s
            .convert(&id, std::slice::from_ref(&target), &StubRunner::new(&[]))
            .expect("convert");

        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].is_created());
        assert_eq!(status_of(&s, &id), DraftStatus::Promoted);
        assert_eq!(conversions_of(&s, &id).len(), 1);
    }

    #[test]
    fn an_invalid_request_runs_nothing_at_all() {
        let s = service();
        let (id, mut target) = convertible(&s, "feature-x");
        target.project_path = "/code/not-registered".into();
        let runner = StubRunner::new(&[]);

        let err = s
            .convert(&id, std::slice::from_ref(&target), &runner)
            .expect_err("must refuse");
        assert!(matches!(err, InboxServiceError::InvalidTargets(_)));
        // The point of validating up front: no worktree was created.
        assert!(runner.created.borrow().is_empty());
        assert_eq!(status_of(&s, &id), DraftStatus::Draft);
        assert!(conversions_of(&s, &id).is_empty());
    }

    #[test]
    fn the_cap_is_enforced_before_anything_runs() {
        let s = service();
        let (id, base) = convertible(&s, "b0");
        let targets: Vec<_> = (0..=MAX_TARGETS)
            .map(|i| ConversionTarget {
                branch: format!("b{i}"),
                ..base.clone()
            })
            .collect();
        let runner = StubRunner::new(&[]);
        assert!(s.convert(&id, &targets, &runner).is_err());
        assert!(runner.created.borrow().is_empty());
    }

    #[test]
    fn a_wave_where_every_target_failed_does_not_promote() {
        let s = service();
        let (id, target) = convertible(&s, "doomed");
        let outcomes = s
            .convert(
                &id,
                std::slice::from_ref(&target),
                &StubRunner::new(&["doomed"]),
            )
            .expect("convert");

        assert!(!outcomes[0].is_created());
        assert_eq!(
            status_of(&s, &id),
            DraftStatus::Draft,
            "nothing was created, so the draft is not promoted"
        );
        // The failure is still on record, with its reason.
        assert_eq!(conversions_of(&s, &id).len(), 1);
    }

    #[test]
    fn a_partial_wave_promotes_and_records_both_sides() {
        let s = service();
        let (id, base) = convertible(&s, "ok");
        let targets = vec![
            base.clone(),
            ConversionTarget {
                branch: "bad".into(),
                ..base.clone()
            },
            ConversionTarget {
                branch: "ok2".into(),
                ..base
            },
        ];
        let outcomes = s
            .convert(&id, &targets, &StubRunner::new(&["bad"]))
            .expect("convert");

        assert_eq!(outcomes.len(), 3);
        assert_eq!(outcomes.iter().filter(|o| o.is_created()).count(), 2);
        assert_eq!(status_of(&s, &id), DraftStatus::Promoted);
        assert_eq!(conversions_of(&s, &id).len(), 3);
    }

    #[test]
    fn a_second_wave_appends_rather_than_replacing_the_first() {
        // The draft is the ledger of everything it ever produced.
        let s = service();
        let (id, base) = convertible(&s, "wave1");
        s.convert(&id, std::slice::from_ref(&base), &StubRunner::new(&[]))
            .expect("first wave");
        let second = ConversionTarget {
            branch: "wave2".into(),
            ..base
        };
        s.convert(&id, std::slice::from_ref(&second), &StubRunner::new(&[]))
            .expect("second wave");

        assert_eq!(conversions_of(&s, &id).len(), 2);
    }

    #[test]
    fn outcomes_are_flushed_as_each_target_finishes() {
        // Crash-safety: the back-link for target one must be on disk before
        // target two is even attempted.
        struct FlushSpy<'a> {
            service: &'a InboxService,
            id: String,
            seen: RefCell<Vec<usize>>,
        }
        impl ConversionRunner for FlushSpy<'_> {
            fn create_worktree(&self, t: &ConversionTarget) -> Result<String, String> {
                // How many back-links are already persisted at this moment?
                let n = match self.service.get(&self.id).expect("get").0 {
                    InboxDraftView::Parsed(d) => d.frontmatter.conversions.len(),
                    _ => 0,
                };
                self.seen.borrow_mut().push(n);
                Ok(format!("/wt/{}", t.branch))
            }
            fn write_note(&self, _p: &str, _b: &str) -> Result<(), String> {
                Ok(())
            }
            fn exclude_note(&self, _p: &str) -> Result<(), String> {
                Ok(())
            }
            fn record_origin(&self, _p: &str, _d: &str) -> Result<(), String> {
                Ok(())
            }
            fn send_prompt(&self, _t: &ConversionTarget, _p: &str) -> Result<(), String> {
                Ok(())
            }
            fn now(&self) -> String {
                "t".into()
            }
        }

        let s = service();
        let (id, base) = convertible(&s, "one");
        let targets = vec![
            base.clone(),
            ConversionTarget {
                branch: "two".into(),
                ..base
            },
        ];
        let spy = FlushSpy {
            service: &s,
            id: id.clone(),
            seen: RefCell::new(Vec::new()),
        };
        s.convert(&id, &targets, &spy).expect("convert");

        assert_eq!(
            spy.seen.into_inner(),
            vec![0, 1],
            "target two should start with target one already recorded"
        );
    }

    #[test]
    fn an_unparseable_draft_cannot_be_converted() {
        let s = service();
        s.projects.add(ProjectEntry {
            path: "/code/acme".into(),
            name: "acme".into(),
            added_at: 0,
        });
        let bad = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        std::fs::create_dir_all(s.store.dir()).expect("dir");
        std::fs::write(
            s.store.dir().join(format!("{bad}.md")),
            "---\nstatus: [oops\n---\nbody\n",
        )
        .expect("write");

        let target = ConversionTarget {
            project_path: "/code/acme".into(),
            branch: "x".into(),
            base_branch: None,
            agent_id: None,
            prompt: "go".into(),
        };
        assert!(s.convert(bad, &[target], &StubRunner::new(&[])).is_err());
    }

    #[test]
    fn conversion_history_survives_a_lost_job() {
        // The job manager is in-memory and dies with the process. What a wave
        // produced must still be knowable afterwards, or a restart strands
        // real worktrees with no record of which draft made them.
        let s = service();
        let (id, base) = convertible(&s, "kept");
        let targets = vec![
            base.clone(),
            ConversionTarget {
                branch: "lost".into(),
                ..base
            },
        ];
        s.convert(&id, &targets, &StubRunner::new(&["lost"]))
            .expect("convert");

        // Nothing in memory is consulted here — this reads the file.
        let fresh = InboxService::new(
            InboxStore::with_dir(s.store.dir().to_path_buf()),
            ProjectsRegistry::with_file(s.projects.file().to_path_buf()),
        );
        let history = fresh.conversion_history(&id).expect("history");

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].branch, "kept");
        assert!(history[0].is_created());
        assert_eq!(history[1].branch, "lost");
        assert!(!history[1].is_created());
        assert_eq!(history[1].error.as_deref(), Some("nope"));
        // The prompt is part of the record: it is the only copy of what was
        // asked of that worktree.
        assert_eq!(history[0].prompt, "build it");
    }

    #[test]
    fn a_partially_written_history_still_reads() {
        let s = service();
        let (id, base) = convertible(&s, "one");
        s.convert(&id, std::slice::from_ref(&base), &StubRunner::new(&[]))
            .expect("convert");

        // Something hand-edited an entry into nonsense. The rest must survive.
        let path = s.store.dir().join(format!("{id}.md"));
        let text = std::fs::read_to_string(&path).expect("read");
        let broken = text.replace("conversions:", "conversions:\n- 42");
        std::fs::write(&path, broken).expect("write");

        let history = s.conversion_history(&id).expect("history");
        assert_eq!(history.len(), 1, "the good entry still reads");
        assert_eq!(history[0].branch, "one");
    }
}

#[cfg(test)]
mod collaboration_tests {
    use super::*;
    use crate::domain::inbox_events::{REDACTED_BODY, RequestStatus};
    use crate::services::inbox_limits::RateLimit;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    #[derive(Default)]
    struct Captured(Mutex<Vec<AuditRecord>>);
    impl AuditSink for Captured {
        fn record(&self, r: &AuditRecord) {
            self.0.lock().unwrap().push(r.clone());
        }
    }
    impl Captured {
        fn all(&self) -> Vec<AuditRecord> {
            self.0.lock().unwrap().clone()
        }
    }

    #[derive(Default)]
    struct Observed(Mutex<Vec<(String, String)>>);
    impl RequestObserver for Observed {
        fn request_opened(&self, draft_id: &str, request_id: &str) {
            self.0
                .lock()
                .unwrap()
                .push((draft_id.to_string(), request_id.to_string()));
        }
    }

    struct Fixture {
        svc: InboxService,
        store: InboxStore,
        audit: Arc<Captured>,
    }

    fn fixture_with(limits: InboxLimits) -> Fixture {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("sebenza-inbox-collab-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("temp base");
        let audit = Arc::new(Captured::default());
        let svc = InboxService::new(
            InboxStore::with_dir(base.join("inbox")),
            ProjectsRegistry::with_file(base.join("projects.json")),
        )
        .with_limits(limits)
        .with_audit_sink(audit.clone());
        Fixture {
            svc,
            store: InboxStore::with_dir(base.join("inbox")),
            audit,
        }
    }

    fn fixture() -> Fixture {
        fixture_with(InboxLimits::default())
    }

    /// Record a created conversion of `id` into `project`/`branch` at `path`.
    fn convert_into(f: &Fixture, id: &str, project: &str, branch: &str, path: &str) {
        let target = ConversionTarget {
            project_path: project.into(),
            branch: branch.into(),
            base_branch: None,
            agent_id: Some("claude".into()),
            prompt: "go".into(),
        };
        let mut all = match f.store.get(id).expect("get") {
            InboxDraftView::Parsed(d) => d.frontmatter.conversions,
            _ => panic!("unparsed"),
        };
        all.push(
            serde_yaml::to_value(ConversionOutcome::created(
                &target,
                path.into(),
                "2026-09-30T00:00:00Z".into(),
            ))
            .unwrap(),
        );
        f.store
            .merge_frontmatter(
                id,
                FrontmatterAuthor::Job,
                FrontmatterPatch {
                    conversions: Some(all),
                    ..Default::default()
                },
            )
            .expect("record conversion");
    }

    fn converted(f: &Fixture) -> String {
        let d = f.svc.create("Idea").expect("create");
        convert_into(
            f,
            &d.id,
            "/code/acme-demo",
            "feat-x",
            "/wt/acme-demo/feat-x",
        );
        d.id
    }

    fn request_from(id: &str, path: &str, branch: &str, body: &str) -> WorktreeIngress {
        WorktreeIngress {
            draft_id: id.into(),
            worktree_path: path.into(),
            branch: branch.into(),
            kind: IngressKind::Request {
                title: Some("Need a decision".into()),
                body: body.into(),
            },
        }
    }

    fn wt() -> WorktreeKey {
        WorktreeKey {
            project: "/code/acme-demo".into(),
            branch: "feat-x".into(),
        }
    }

    fn fm(f: &Fixture, id: &str) -> crate::domain::model::InboxDraftFrontmatter {
        match f.store.get(id).expect("get") {
            InboxDraftView::Parsed(d) => d.frontmatter,
            _ => panic!("unparsed"),
        }
    }

    // --- Priority (TS-06, TS-32) -------------------------------------------

    // TS-06: P2 by agent; override to P0; clearing hands control back.
    #[test]
    fn an_override_is_sticky_until_cleared() {
        let f = fixture();
        let id = f.svc.create("Ranked").expect("create").id;
        assert_eq!(fm(&f, &id).priority, Priority::P2);
        assert_eq!(fm(&f, &id).priority_source, PrioritySource::Agent);

        let d = f
            .svc
            .set_priority(&id, Some(Priority::P0), None)
            .expect("set");
        assert_eq!(d.frontmatter.priority, Priority::P0);
        assert_eq!(d.frontmatter.priority_source, PrioritySource::Operator);

        assert!(
            f.svc
                .set_agent_priority(&id, Priority::P3)
                .expect("agent")
                .is_none(),
            "the agent may not move an operator override"
        );
        assert_eq!(fm(&f, &id).priority, Priority::P0);

        let d = f.svc.set_priority(&id, None, None).expect("clear");
        assert_eq!(
            d.frontmatter.priority,
            Priority::P0,
            "clearing keeps the value"
        );
        assert_eq!(d.frontmatter.priority_source, PrioritySource::Agent);

        assert!(
            f.svc
                .set_agent_priority(&id, Priority::P3)
                .expect("agent")
                .is_some()
        );
        assert_eq!(fm(&f, &id).priority, Priority::P3);
    }

    // TS-06 / TS-40: every operator priority change is audited, clear included.
    #[test]
    fn priority_changes_are_audited_as_operator() {
        let f = fixture();
        let id = f.svc.create("Ranked").expect("create").id;
        f.svc
            .set_priority(&id, Some(Priority::P1), None)
            .expect("set");
        f.svc.set_priority(&id, None, None).expect("clear");

        let records = f.audit.all();
        let actions: Vec<_> = records.iter().map(|r| r.action).collect();
        assert_eq!(
            actions,
            ["inbox.priority.changed", "inbox.priority.override_cleared"]
        );
        assert!(records.iter().all(|r| r.actor == AuthorKind::Operator));
        assert_eq!(records[0].from, Some(Priority::P2));
        assert_eq!(records[0].to, Some(Priority::P1));
        assert_eq!(records[0].source, Some(PrioritySource::Operator));
        assert!(records[0].event_id.is_some());
    }

    // TS-32: a worktree holding the token succeeds (accepted T-01); the audit
    // and the event both say operator, plus the unauthenticated marker.
    #[test]
    fn a_worktree_caller_on_priority_is_recorded_as_operator_with_its_marker() {
        let f = fixture();
        let id = f.svc.create("Ranked").expect("create").id;
        f.svc
            .set_priority(&id, Some(Priority::P0), Some("worktree".into()))
            .expect("accepted residual: this succeeds");

        let r = &f.audit.all()[0];
        assert_eq!(r.actor, AuthorKind::Operator);
        assert_eq!(r.caller.as_deref(), Some("worktree"));

        let events = f.svc.events(&id).expect("events");
        let last = events.last().expect("priority_changed");
        assert_eq!(last.author, AuthorKind::Operator);
        assert_eq!(last.caller.as_deref(), Some("worktree"));
    }

    #[test]
    fn the_list_is_ordered_by_priority_then_newest() {
        let f = fixture();
        let older = f.svc.create("Older").expect("create").id;
        f.svc.create("Newer").expect("create");
        f.svc.create("Newest").expect("create");
        // The oldest draft becomes the most urgent and jumps the queue.
        f.svc
            .set_priority(&older, Some(Priority::P0), None)
            .expect("set");

        let listed = f.svc.list(&ListQuery::default()).expect("list");
        assert_eq!(listed[0].id, older);
        assert_eq!(listed[0].priority, Priority::P0);
        assert_eq!(listed[0].priority_source, PrioritySource::Operator);
        assert!(listed[1..].iter().all(|d| d.priority == Priority::P2));
    }

    // --- Comments ------------------------------------------------------------

    #[test]
    fn comments_group_into_overall_and_per_worktree() {
        let f = fixture();
        let id = converted(&f);
        f.svc
            .add_comment(
                &id,
                EventAuthor::operator(),
                Thread::Overall,
                "overall note",
            )
            .expect("overall");
        f.svc
            .add_comment(
                &id,
                EventAuthor::operator(),
                Thread::Worktree(wt()),
                "worktree note",
            )
            .expect("worktree");
        f.svc
            .ingest(&WorktreeIngress {
                draft_id: id.clone(),
                worktree_path: "/wt/acme-demo/feat-x".into(),
                branch: "feat-x".into(),
                kind: IngressKind::Comment {
                    body: "from the agent".into(),
                },
            })
            .expect("agent comment");

        let groups = f.svc.list_comments(&id).expect("groups");
        assert_eq!(groups.overall.len(), 1);
        assert_eq!(groups.overall[0].body, "overall note");
        assert_eq!(groups.overall[0].kind, CommentKind::Note);
        assert_eq!(groups.worktrees.len(), 1);
        let g = &groups.worktrees[0];
        assert_eq!(
            (g.project.as_str(), g.branch.as_str()),
            ("/code/acme-demo", "feat-x")
        );
        let bodies: Vec<_> = g.comments.iter().map(|c| c.body.as_str()).collect();
        assert_eq!(bodies, ["worktree note", "from the agent"]);
        assert_eq!(g.comments[1].author, AuthorKind::WorktreeAgent);
        assert_eq!(g.comments[1].caller.as_deref(), Some("worktree"));
    }

    #[test]
    fn a_converted_worktree_has_a_group_before_anyone_comments() {
        let f = fixture();
        let id = converted(&f);
        let groups = f.svc.list_comments(&id).expect("groups");
        assert_eq!(groups.worktrees.len(), 1);
        assert!(groups.worktrees[0].comments.is_empty());
    }

    #[test]
    fn a_worktree_comment_must_name_a_converted_worktree() {
        let f = fixture();
        let id = converted(&f);
        let err = f
            .svc
            .add_comment(
                &id,
                EventAuthor::operator(),
                Thread::Worktree(WorktreeKey {
                    project: "/code/elsewhere".into(),
                    branch: "nope".into(),
                }),
                "stray",
            )
            .expect_err("unknown worktree");
        assert!(matches!(err, InboxServiceError::Invalid(_)), "{err:?}");
        assert!(f.svc.events(&id).expect("events").is_empty());
    }

    #[test]
    fn a_blank_comment_is_refused() {
        let f = fixture();
        let id = f.svc.create("x").expect("create").id;
        let err = f
            .svc
            .add_comment(&id, EventAuthor::operator(), Thread::Overall, "   ")
            .expect_err("blank");
        assert!(matches!(err, InboxServiceError::Invalid(_)), "{err:?}");
    }

    #[test]
    fn a_redacted_comment_reads_back_masked() {
        let f = fixture();
        let id = f.svc.create("x").expect("create").id;
        let e = f
            .svc
            .add_comment(
                &id,
                EventAuthor::operator(),
                Thread::Overall,
                "oops a secret",
            )
            .expect("comment");
        f.store
            .append_event(
                &id,
                EventAuthor::operator(),
                None,
                InboxEventKind::Redacted {
                    target_event_id: e.event_id.clone(),
                },
            )
            .expect("redact");
        let groups = f.svc.list_comments(&id).expect("groups");
        assert_eq!(groups.overall.len(), 1, "the tombstone is not itself a row");
        assert_eq!(groups.overall[0].body, REDACTED_BODY);
        assert!(groups.overall[0].redacted);
    }

    #[test]
    fn a_secret_in_a_comment_is_kept_and_flagged() {
        let f = fixture();
        let id = f.svc.create("x").expect("create").id;
        let body = "token sk-TEST-0000000000000000000000000000";
        f.svc
            .add_comment(&id, EventAuthor::operator(), Thread::Overall, body)
            .expect("comment");
        let row = &f.svc.list_comments(&id).expect("groups").overall[0];
        assert_eq!(row.body, body, "the scan warns; it never rewrites");
        assert!(!row.warnings.is_empty());
    }

    // --- Requests (TS-11, TS-12, TS-37) -------------------------------------

    // TS-11: a request from a converted worktree lands open in its group.
    #[test]
    fn a_request_from_a_converted_worktree_opens_in_its_group() {
        let f = fixture();
        let observed = Arc::new(Observed::default());
        f.svc.set_request_observer(observed.clone());
        let id = converted(&f);

        let event = f
            .svc
            .ingest(&request_from(
                &id,
                "/wt/acme-demo/feat-x",
                "feat-x",
                "Which db?",
            ))
            .expect("ingest");
        assert_eq!(event.author, AuthorKind::WorktreeAgent);

        let requests = f.svc.list_requests(&id).expect("requests");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].status, RequestStatus::Open);
        assert_eq!(requests[0].worktree, wt());
        assert_eq!(requests[0].title, "Need a decision");

        let groups = f.svc.list_comments(&id).expect("groups");
        let row = &groups.worktrees[0].comments[0];
        assert_eq!(row.kind, CommentKind::Request);
        assert_eq!(
            row.request_id.as_deref(),
            Some(requests[0].request_id.as_str())
        );

        let seen = observed.0.lock().unwrap().clone();
        assert_eq!(seen, vec![(id, requests[0].request_id.clone())]);
    }

    #[test]
    fn a_trailing_slash_on_the_claimed_path_still_matches() {
        let f = fixture();
        let id = converted(&f);
        f.svc
            .ingest(&request_from(&id, "/wt/acme-demo/feat-x/", "feat-x", "b"))
            .expect("ingest");
    }

    #[test]
    fn a_request_without_a_title_takes_its_first_line() {
        let f = fixture();
        let id = converted(&f);
        f.svc
            .ingest(&WorktreeIngress {
                draft_id: id.clone(),
                worktree_path: "/wt/acme-demo/feat-x".into(),
                branch: "feat-x".into(),
                kind: IngressKind::Request {
                    title: None,
                    body: "Which database?\nDetails follow.".into(),
                },
            })
            .expect("ingest");
        assert_eq!(
            f.svc.list_requests(&id).unwrap()[0].title,
            "Which database?"
        );
    }

    // TS-12: a worktree not in conversions[] is refused; nothing appended.
    #[test]
    fn a_request_from_a_foreign_worktree_is_refused() {
        let f = fixture();
        let id = converted(&f);
        for (path, branch) in [
            ("/wt/somewhere/else", "feat-x"),
            // Right path, wrong branch: control.env and the record disagree.
            ("/wt/acme-demo/feat-x", "other"),
        ] {
            let err = f
                .svc
                .ingest(&request_from(&id, path, branch, "b"))
                .expect_err("foreign");
            assert!(
                matches!(err, InboxServiceError::ForeignWorktree(_)),
                "{err:?}"
            );
        }
        assert!(f.svc.events(&id).expect("events").is_empty());
    }

    #[test]
    fn a_failed_conversion_does_not_vouch_for_a_worktree() {
        let f = fixture();
        let id = f.svc.create("Idea").expect("create").id;
        let target = ConversionTarget {
            project_path: "/code/acme-demo".into(),
            branch: "feat-x".into(),
            base_branch: None,
            agent_id: None,
            prompt: "go".into(),
        };
        let mut failed = ConversionOutcome::failed(&target, "boom".into(), "t".into());
        failed.worktree_path = Some("/wt/acme-demo/feat-x".into());
        f.store
            .merge_frontmatter(
                &id,
                FrontmatterAuthor::Job,
                FrontmatterPatch {
                    conversions: Some(vec![serde_yaml::to_value(failed).unwrap()]),
                    ..Default::default()
                },
            )
            .expect("record");
        assert!(matches!(
            f.svc
                .ingest(&request_from(&id, "/wt/acme-demo/feat-x", "feat-x", "b")),
            Err(InboxServiceError::ForeignWorktree(_))
        ));
    }

    // TS-37: a forged inbox-origin.json naming another item is refused by the
    // path cross-check.
    #[test]
    fn a_forged_origin_naming_another_item_is_refused() {
        let f = fixture();
        let mine = converted(&f);
        let theirs = f.svc.create("Someone else's").expect("create").id;
        convert_into(&f, &theirs, "/code/beta", "feat-y", "/wt/beta/feat-y");

        let err = f
            .svc
            .ingest(&request_from(
                &theirs,
                "/wt/acme-demo/feat-x",
                "feat-x",
                "b",
            ))
            .expect_err("forged origin");
        assert!(
            matches!(err, InboxServiceError::ForeignWorktree(_)),
            "{err:?}"
        );
        assert!(f.svc.events(&theirs).expect("events").is_empty());
        assert!(f.svc.events(&mine).expect("events").is_empty());
    }

    // --- Limits (TS-13) ------------------------------------------------------

    fn tight(requests: usize, comments: usize, open: usize) -> InboxLimits {
        InboxLimits {
            max_open_requests: open,
            requests: RateLimit {
                max: requests,
                window: Duration::from_secs(60),
            },
            comments: RateLimit {
                max: comments,
                window: Duration::from_secs(60),
            },
            ..InboxLimits::default()
        }
    }

    #[test]
    fn an_oversized_body_or_title_is_refused() {
        let f = fixture();
        let id = converted(&f);
        let huge = "x".repeat(f.svc.limits.max_body_bytes + 1);
        assert!(matches!(
            f.svc
                .ingest(&request_from(&id, "/wt/acme-demo/feat-x", "feat-x", &huge)),
            Err(InboxServiceError::TooLarge { .. })
        ));
        assert!(matches!(
            f.svc
                .add_comment(&id, EventAuthor::operator(), Thread::Overall, &huge),
            Err(InboxServiceError::TooLarge { .. })
        ));
        let long_title = WorktreeIngress {
            kind: IngressKind::Request {
                title: Some("t".repeat(f.svc.limits.max_title_chars + 1)),
                body: "b".into(),
            },
            ..request_from(&id, "/wt/acme-demo/feat-x", "feat-x", "b")
        };
        assert!(matches!(
            f.svc.ingest(&long_title),
            Err(InboxServiceError::TooLarge { .. })
        ));
        assert!(f.svc.events(&id).expect("events").is_empty());
    }

    #[test]
    fn a_request_flood_is_rate_limited_per_worktree() {
        let f = fixture_with(tight(2, 100, 100));
        let id = converted(&f);
        convert_into(&f, &id, "/code/acme-demo", "feat-z", "/wt/acme-demo/feat-z");
        let from_x = request_from(&id, "/wt/acme-demo/feat-x", "feat-x", "b");
        f.svc.ingest(&from_x).expect("1");
        f.svc.ingest(&from_x).expect("2");
        assert!(matches!(
            f.svc.ingest(&from_x),
            Err(InboxServiceError::RateLimited)
        ));
        // Another worktree is not starved by the first one's flood.
        f.svc
            .ingest(&request_from(&id, "/wt/acme-demo/feat-z", "feat-z", "b"))
            .expect("other worktree");
        assert_eq!(f.svc.list_requests(&id).unwrap().len(), 3);
    }

    #[test]
    fn a_comment_flood_is_rate_limited() {
        let f = fixture_with(tight(100, 2, 100));
        let id = converted(&f);
        let c = WorktreeIngress {
            kind: IngressKind::Comment { body: "c".into() },
            ..request_from(&id, "/wt/acme-demo/feat-x", "feat-x", "b")
        };
        f.svc.ingest(&c).expect("1");
        f.svc.ingest(&c).expect("2");
        assert!(matches!(
            f.svc.ingest(&c),
            Err(InboxServiceError::RateLimited)
        ));
    }

    #[test]
    fn open_requests_per_item_are_bounded() {
        let f = fixture_with(tight(100, 100, 2));
        let id = converted(&f);
        let r = request_from(&id, "/wt/acme-demo/feat-x", "feat-x", "b");
        f.svc.ingest(&r).expect("1");
        f.svc.ingest(&r).expect("2");
        assert!(matches!(
            f.svc.ingest(&r),
            Err(InboxServiceError::TooManyOpenRequests(_))
        ));
        assert_eq!(f.svc.list_requests(&id).unwrap().len(), 2, "depth bounded");
    }

    // --- Audit (TS-40, TS-32) -------------------------------------------------

    #[test]
    fn ingress_and_comments_are_audited_metadata_only() {
        let f = fixture();
        let id = converted(&f);
        let secret = "sk-TEST-0000000000000000 PLEASE-DO-NOT-LOG";
        f.svc
            .add_comment(&id, EventAuthor::operator(), Thread::Overall, secret)
            .expect("comment");
        f.svc
            .ingest(&request_from(&id, "/wt/acme-demo/feat-x", "feat-x", secret))
            .expect("request");
        f.svc
            .ingest(&WorktreeIngress {
                kind: IngressKind::Comment {
                    body: secret.into(),
                },
                ..request_from(&id, "/wt/acme-demo/feat-x", "feat-x", "b")
            })
            .expect("agent comment");

        let records = f.audit.all();
        let actions: Vec<_> = records.iter().map(|r| r.action).collect();
        assert_eq!(
            actions,
            [
                "inbox.comment.added",
                "inbox.request.opened",
                "inbox.comment.added"
            ]
        );
        for r in &records {
            let json = serde_json::to_string(r).unwrap();
            assert!(!json.contains("PLEASE-DO-NOT-LOG"), "body leaked: {json}");
            assert!(!json.contains("Need a decision"), "title leaked: {json}");
            assert!(r.event_id.is_some());
        }
        assert_eq!(records[0].actor, AuthorKind::Operator);
        assert_eq!(records[0].thread, Some("overall"));
        // The worktree path is accepted as declared: say so in the record.
        assert_eq!(records[1].actor, AuthorKind::WorktreeAgent);
        assert_eq!(records[1].caller.as_deref(), Some("worktree"));
        assert_eq!(records[1].branch.as_deref(), Some("feat-x"));
        assert!(records[1].request_id.is_some());
        assert_eq!(records[2].thread, Some("worktree"));
    }

    #[test]
    fn a_refused_ingress_is_not_audited_as_accepted() {
        let f = fixture();
        let id = converted(&f);
        let _ = f
            .svc
            .ingest(&request_from(&id, "/wt/nowhere", "feat-x", "b"));
        assert!(
            f.audit
                .all()
                .iter()
                .all(|r| r.action != "inbox.request.opened")
        );
    }

    // --- Parsing ---------------------------------------------------------------

    #[test]
    fn non_inbox_runtime_events_are_not_ingress() {
        let raw = serde_json::json!({"type": "agent_stopped", "worktreeId": "w", "branch": "b"});
        assert!(parse_worktree_ingress(&raw).is_none());
    }

    #[test]
    fn a_request_event_parses() {
        let raw = serde_json::json!({
            "type": "inbox.request", "worktreeId": "w", "branch": "feat-x",
            "draftId": "01ARZ3NDEKTSV4RRFFQ69G5FAV", "worktreePath": "/wt/x",
            "title": "T", "body": "B",
        });
        let got = parse_worktree_ingress(&raw).expect("inbox").expect("valid");
        assert_eq!(got.draft_id, "01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert_eq!(got.worktree_path, "/wt/x");
        assert_eq!(got.branch, "feat-x");
        assert_eq!(
            got.kind,
            IngressKind::Request {
                title: Some("T".into()),
                body: "B".into()
            }
        );
    }

    #[test]
    fn a_comment_event_parses() {
        let raw = serde_json::json!({
            "type": "inbox.comment", "worktreeId": "w", "branch": "b",
            "draftId": "01ARZ3NDEKTSV4RRFFQ69G5FAV", "worktreePath": "/wt/x", "body": "B",
        });
        let got = parse_worktree_ingress(&raw).expect("inbox").expect("valid");
        assert_eq!(got.kind, IngressKind::Comment { body: "B".into() });
    }

    #[test]
    fn a_malformed_inbox_event_is_an_error_not_a_runtime_event() {
        for raw in [
            serde_json::json!({"type": "inbox.request", "branch": "b", "worktreePath": "/x", "body": "B"}),
            serde_json::json!({"type": "inbox.request", "branch": "b", "draftId": "D", "body": "B"}),
            serde_json::json!({"type": "inbox.request", "draftId": "D", "worktreePath": "/x", "body": "B"}),
            serde_json::json!({"type": "inbox.comment", "branch": "b", "draftId": "D", "worktreePath": "/x"}),
            serde_json::json!({"type": "inbox.frobnicate", "branch": "b", "draftId": "D", "worktreePath": "/x", "body": "B"}),
        ] {
            assert!(
                matches!(parse_worktree_ingress(&raw), Some(Err(_))),
                "must reject {raw}"
            );
        }
    }
}
