//! Filesystem store for inbox drafts: `~/.ai/sebenza/inbox/<ulid>.md`.
//!
//! Writes are temp-file-then-rename. Body saves are gated on `body_hash`.
//! Frontmatter merges are key-scoped by author so a conversion job cannot
//! clobber the title and an editor cannot drop `conversions[]`.

use crate::domain::inbox_events::{
    AgentSession, AuthorKind, INBOX_EVENT_SCHEMA_VERSION, InboxEvent, InboxEventKind,
};
use crate::domain::model::{
    DraftStatus, FileRevision, INBOX_DRAFT_SCHEMA_VERSION, InboxDraft, InboxDraftFrontmatter,
    InboxDraftView, Priority, PrioritySource, ProjectRef, parse_inbox_file, render_inbox_file,
};
use crate::util::id::{is_ulid, random_ulid};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Every way a store operation can fail. `Conflict` carries both hashes so a
/// caller can show the editor what it was racing.
#[derive(Debug, Error)]
pub enum InboxStoreError {
    #[error("draft {0} not found")]
    NotFound(String),
    #[error("invalid draft id {0}")]
    InvalidId(String),
    #[error("draft {id} is not parseable ({error})")]
    Unparsed { id: String, error: String },
    #[error("body conflict for {id}")]
    Conflict {
        id: String,
        expected: String,
        actual: String,
    },
    #[error("draft {id} has schema_version {version}, newer than this binary supports")]
    UnsupportedVersion { id: String, version: i32 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A priority write. The operator sets or clears an override; the agent's
/// write is ignored while an operator override stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityWrite {
    /// `Some` sets a sticky override; `None` clears it and hands control back.
    Operator(Option<Priority>),
    Agent(Priority),
}

/// Who wrote an event, plus the self-declared caller marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventAuthor {
    pub kind: AuthorKind,
    pub caller: Option<String>,
}

impl EventAuthor {
    pub fn operator() -> Self {
        Self {
            kind: AuthorKind::Operator,
            caller: None,
        }
    }
    pub fn system_agent() -> Self {
        Self {
            kind: AuthorKind::SystemAgent,
            caller: None,
        }
    }
    pub fn worktree_agent() -> Self {
        Self {
            kind: AuthorKind::WorktreeAgent,
            caller: Some("worktree".into()),
        }
    }
}

/// Who is writing frontmatter. Each author owns a disjoint key set, except
/// `Promoted` which the job sets and which wins a race with `Dropped`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontmatterAuthor {
    /// Owns `title`, `project` and the `Draft -> Dropped` transition.
    Editor,
    /// Owns `conversions[]` and the `Draft -> Promoted` transition.
    Job,
}

/// A sparse frontmatter edit. `None` leaves the key untouched; keys the
/// `author` does not own are ignored rather than rejected.
#[derive(Debug, Clone, Default)]
pub struct FrontmatterPatch {
    /// Rename the draft. The title never reaches the filename.
    pub title: Option<String>,
    /// `Some(None)` unlinks the project.
    pub project: Option<Option<ProjectRef>>,
    /// Requested status transition; see [`FrontmatterAuthor`] for who may set which.
    pub status: Option<DraftStatus>,
    /// Back-links appended by a conversion job, replacing the array wholesale.
    pub conversions: Option<Vec<serde_yaml::Value>>,
}

/// Reads and writes drafts in one directory. Holds no cache and no lock — the
/// filesystem is the source of truth, so a second server or an external editor
/// may write the same files concurrently.
pub struct InboxStore {
    dir: PathBuf,
}

fn default_inbox_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home)
        .join(".ai")
        .join("sebenza")
        .join("inbox")
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

impl InboxStore {
    /// A store over `~/.ai/sebenza/inbox/`, the global location.
    pub fn new() -> Self {
        Self {
            dir: default_inbox_dir(),
        }
    }

    /// A store over an explicit directory. Used by tests.
    pub fn with_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The directory this store reads and writes.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_for(&self, id: &str) -> Result<PathBuf, InboxStoreError> {
        if !is_ulid(id) {
            return Err(InboxStoreError::InvalidId(id.to_string()));
        }
        Ok(self.dir.join(format!("{id}.md")))
    }

    fn atomic_write(&self, id: &str, contents: &str) -> Result<(), InboxStoreError> {
        fs::create_dir_all(&self.dir)?;
        let final_path = self.path_for(id)?;
        let tmp_path = self.dir.join(format!(
            "{id}.{}.{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::write(&tmp_path, contents)?;
        fs::rename(&tmp_path, &final_path)?;
        Ok(())
    }

    fn read_view(&self, id: &str) -> Result<InboxDraftView, InboxStoreError> {
        let path = self.path_for(id)?;
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(InboxStoreError::NotFound(id.to_string()));
            }
            Err(e) => return Err(e.into()),
        };
        Ok(parse_inbox_file(id, &text))
    }

    fn require_parsed(&self, id: &str) -> Result<InboxDraft, InboxStoreError> {
        match self.read_view(id)? {
            InboxDraftView::Parsed(d) => Ok(d),
            InboxDraftView::Raw { error, .. } => Err(InboxStoreError::Unparsed {
                id: id.to_string(),
                error,
            }),
        }
    }

    /// Create an empty draft under a fresh ULID. The title is frontmatter only,
    /// so no user text reaches the path.
    pub fn create(&self, title: &str) -> Result<InboxDraft, InboxStoreError> {
        let id = random_ulid();
        let now = now_rfc3339();
        let draft = InboxDraft {
            id: id.clone(),
            frontmatter: InboxDraftFrontmatter {
                schema_version: INBOX_DRAFT_SCHEMA_VERSION,
                title: title.to_string(),
                project: None,
                status: DraftStatus::Draft,
                created_at: now.clone(),
                updated_at: now,
                conversions: Vec::new(),
                priority: Priority::default(),
                priority_source: PrioritySource::default(),
                extra: serde_yaml::Mapping::new(),
            },
            body: String::new(),
        };
        self.atomic_write(&id, &render_inbox_file(&draft))?;
        Ok(draft)
    }

    /// Read one draft. Unparseable frontmatter comes back as
    /// [`InboxDraftView::Raw`] rather than an error.
    pub fn get(&self, id: &str) -> Result<InboxDraftView, InboxStoreError> {
        self.read_view(id)
    }

    /// Every draft in the directory, newest ULID first. Non-`.md` files and
    /// non-ULID names are skipped; one bad draft never hides the rest, and a
    /// missing directory lists empty.
    pub fn list(&self) -> Result<Vec<InboxDraftView>, InboxStoreError> {
        let mut out = Vec::new();
        let entries = match fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        for ent in entries.flatten() {
            let name = ent.file_name();
            let name = name.to_string_lossy();
            let Some(id) = name.strip_suffix(".md") else {
                continue;
            };
            if !is_ulid(id) {
                continue;
            }
            match fs::read_to_string(ent.path()) {
                Ok(text) => out.push(parse_inbox_file(id, &text)),
                Err(_) => continue,
            }
        }
        out.sort_by(|a, b| view_id(b).cmp(view_id(a)));
        Ok(out)
    }

    /// Replace the body if `expected_hash` matches the on-disk body. Frontmatter
    /// is left alone aside from `updated_at`.
    pub fn save_body(
        &self,
        id: &str,
        expected_hash: &str,
        body: &str,
    ) -> Result<InboxDraft, InboxStoreError> {
        let mut draft = self.require_parsed(id)?;
        let actual = FileRevision::of_body(&draft.body);
        if actual.body_hash != expected_hash {
            return Err(InboxStoreError::Conflict {
                id: id.to_string(),
                expected: expected_hash.to_string(),
                actual: actual.body_hash,
            });
        }
        draft.body = body.to_string();
        draft.frontmatter.updated_at = now_rfc3339();
        self.atomic_write(id, &render_inbox_file(&draft))?;
        Ok(draft)
    }

    /// Rewrite the body without a hash check. Frontmatter (including
    /// `conversions[]`) is re-read and written back unchanged except `updated_at`.
    pub fn force_write_body(&self, id: &str, body: &str) -> Result<InboxDraft, InboxStoreError> {
        let mut draft = self.require_parsed(id)?;
        draft.body = body.to_string();
        draft.frontmatter.updated_at = now_rfc3339();
        self.atomic_write(id, &render_inbox_file(&draft))?;
        Ok(draft)
    }

    /// Remove the draft file. Unparseable drafts delete like any other — a
    /// malformed file must not become undeletable.
    pub fn delete(&self, id: &str) -> Result<(), InboxStoreError> {
        let path = self.path_for(id)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(InboxStoreError::NotFound(id.to_string()))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Apply `patch`, keeping only the keys `author` owns, then write the whole
    /// file back. This is how a job appends a back-link without touching the
    /// body and how the editor renames without dropping `conversions[]`.
    pub fn merge_frontmatter(
        &self,
        id: &str,
        author: FrontmatterAuthor,
        patch: FrontmatterPatch,
    ) -> Result<InboxDraft, InboxStoreError> {
        let mut draft = self.require_parsed(id)?;
        apply_patch(&mut draft.frontmatter, author, patch);
        draft.frontmatter.updated_at = now_rfc3339();
        self.atomic_write(id, &render_inbox_file(&draft))?;
        Ok(draft)
    }
}

impl InboxStore {
    /// Set or clear priority. Returns the `priority_changed` event appended,
    /// or `None` when nothing changed (including an agent write blocked by an
    /// operator override).
    pub fn set_priority(
        &self,
        id: &str,
        write: PriorityWrite,
    ) -> Result<Option<InboxEvent>, InboxStoreError> {
        let _ = (id, write);
        todo!("set_priority")
    }

    /// Append one event to `<id>.events.jsonl`. The draft must exist.
    pub fn append_event(
        &self,
        id: &str,
        author: EventAuthor,
        parent_event_id: Option<String>,
        kind: InboxEventKind,
    ) -> Result<InboxEvent, InboxStoreError> {
        let _ = (
            id,
            author,
            parent_event_id,
            kind,
            INBOX_EVENT_SCHEMA_VERSION,
        );
        todo!("append_event")
    }

    /// Every parseable event, in append order. A torn or foreign line is skipped.
    pub fn read_events(&self, id: &str) -> Result<Vec<InboxEvent>, InboxStoreError> {
        let _ = id;
        todo!("read_events")
    }

    pub fn read_session(&self, id: &str) -> Result<Option<AgentSession>, InboxStoreError> {
        let _ = id;
        todo!("read_session")
    }

    pub fn write_session(&self, id: &str, session: &AgentSession) -> Result<(), InboxStoreError> {
        let _ = (id, session);
        todo!("write_session")
    }

    /// Remove sidecars whose draft no longer exists. Returns what was removed.
    pub fn sweep_orphans(&self) -> Result<Vec<PathBuf>, InboxStoreError> {
        todo!("sweep_orphans")
    }
}

fn view_id(v: &InboxDraftView) -> &str {
    match v {
        InboxDraftView::Parsed(d) => d.id.as_str(),
        InboxDraftView::Raw { id, .. } => id.as_str(),
    }
}

fn apply_patch(fm: &mut InboxDraftFrontmatter, author: FrontmatterAuthor, patch: FrontmatterPatch) {
    match author {
        FrontmatterAuthor::Editor => {
            if let Some(title) = patch.title {
                fm.title = title;
            }
            if let Some(project) = patch.project {
                fm.project = project;
            }
            if let Some(status) = patch.status {
                // Job owns Promoted; an editor Dropped cannot overwrite it.
                if fm.status != DraftStatus::Promoted && status != DraftStatus::Promoted {
                    fm.status = status;
                }
            }
        }
        FrontmatterAuthor::Job => {
            if let Some(conversions) = patch.conversions {
                fm.conversions = conversions;
            }
            if patch.status == Some(DraftStatus::Promoted) {
                fm.status = DraftStatus::Promoted;
            }
        }
    }
}

impl Default for InboxStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn store() -> InboxStore {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("sebenza-inbox-store-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp inbox dir");
        InboxStore::with_dir(dir)
    }

    fn parsed(view: InboxDraftView) -> InboxDraft {
        match view {
            InboxDraftView::Parsed(d) => d,
            other => panic!("expected parsed, got {other:?}"),
        }
    }

    #[test]
    fn create_uses_ulid_filename() {
        let s = store();
        let draft = s.create("Hello").expect("create");
        assert!(is_ulid(&draft.id), "id should be a ULID, got {}", draft.id);
        let path = s.dir().join(format!("{}.md", draft.id));
        assert!(
            path.is_file(),
            "draft file should exist at {}",
            path.display()
        );
        assert_eq!(draft.frontmatter.title, "Hello");
        assert_eq!(draft.frontmatter.status, DraftStatus::Draft);
        assert!(draft.frontmatter.conversions.is_empty());
        assert!(s.dir().read_dir().unwrap().all(|e| {
            let n = e.unwrap().file_name();
            let n = n.to_string_lossy();
            n.ends_with(".md") && !n.ends_with(".tmp")
        }));
    }

    #[test]
    fn list_includes_raw_and_skips_non_md() {
        let s = store();
        let a = s.create("A").expect("a");
        fs::write(s.dir().join("notes.txt"), "ignore me").unwrap();
        fs::write(
            s.dir().join("01ARZ3NDEKTSV4RRFFQ69G5FAV.md"),
            "---\nstatus: [unterminated\n---\nbody\n",
        )
        .unwrap();
        let listed = s.list().expect("list");
        let ids: Vec<String> = listed
            .iter()
            .map(|v| match v {
                InboxDraftView::Parsed(d) => d.id.clone(),
                InboxDraftView::Raw { id, .. } => id.clone(),
            })
            .collect();
        assert!(ids.contains(&a.id));
        assert!(ids.iter().any(|id| id == "01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert_eq!(listed.len(), 2);
        assert!(
            listed
                .iter()
                .any(|v| matches!(v, InboxDraftView::Raw { .. }))
        );
    }

    #[test]
    fn save_body_conflicts_on_stale_hash() {
        let s = store();
        let draft = s.create("T").expect("create");
        let hash = FileRevision::of_body(&draft.body).body_hash;
        s.save_body(&draft.id, &hash, "new body\n")
            .expect("first save");
        let err = s
            .save_body(&draft.id, &hash, "other\n")
            .expect_err("stale hash must conflict");
        match err {
            InboxStoreError::Conflict { id, .. } => assert_eq!(id, draft.id),
            other => panic!("expected Conflict, got {other:?}"),
        }
        let current = parsed(s.get(&draft.id).unwrap());
        assert_eq!(current.body, "new body\n");
    }

    #[test]
    fn editor_merge_does_not_drop_job_conversions() {
        let s = store();
        let draft = s.create("T").expect("create");
        let conv: Vec<serde_yaml::Value> =
            serde_yaml::from_str("- {branch: feat-x}").expect("yaml");
        s.merge_frontmatter(
            &draft.id,
            FrontmatterAuthor::Job,
            FrontmatterPatch {
                conversions: Some(conv.clone()),
                status: Some(DraftStatus::Promoted),
                ..FrontmatterPatch::default()
            },
        )
        .expect("job merge");
        s.merge_frontmatter(
            &draft.id,
            FrontmatterAuthor::Editor,
            FrontmatterPatch {
                title: Some("Renamed".into()),
                status: Some(DraftStatus::Dropped),
                conversions: Some(Vec::new()),
                ..FrontmatterPatch::default()
            },
        )
        .expect("editor merge");
        let current = parsed(s.get(&draft.id).unwrap());
        assert_eq!(current.frontmatter.title, "Renamed");
        assert_eq!(current.frontmatter.status, DraftStatus::Promoted);
        assert_eq!(current.frontmatter.conversions, conv);
    }

    #[test]
    fn delete_removes_parsed_and_malformed_drafts_alike() {
        let s = store();
        let draft = s.create("Goodbye").expect("create");
        s.delete(&draft.id).expect("delete parsed");
        assert!(matches!(
            s.get(&draft.id),
            Err(InboxStoreError::NotFound(_))
        ));
        assert!(matches!(
            s.delete(&draft.id),
            Err(InboxStoreError::NotFound(_))
        ));

        // A draft that does not parse must still be removable, or a malformed
        // file would be stuck in the inbox forever.
        let bad = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        fs::write(
            s.dir().join(format!("{bad}.md")),
            "---\nstatus: [oops\n---\nx\n",
        )
        .expect("write malformed");
        s.delete(bad).expect("delete malformed");
        assert!(s.list().expect("list").is_empty());

        assert!(matches!(
            s.delete("../escape"),
            Err(InboxStoreError::InvalidId(_))
        ));
    }

    #[test]
    fn force_write_body_preserves_conversions() {
        let s = store();
        let draft = s.create("T").expect("create");
        let conv: Vec<serde_yaml::Value> = serde_yaml::from_str("- {ok: true}").expect("yaml");
        s.merge_frontmatter(
            &draft.id,
            FrontmatterAuthor::Job,
            FrontmatterPatch {
                conversions: Some(conv.clone()),
                ..FrontmatterPatch::default()
            },
        )
        .expect("job");
        s.force_write_body(&draft.id, "forced\n").expect("force");
        let current = parsed(s.get(&draft.id).unwrap());
        assert_eq!(current.body, "forced\n");
        assert_eq!(current.frontmatter.conversions, conv);
    }

    #[test]
    fn traversal_id_is_rejected() {
        let s = store();
        let err = s.get("../secret").expect_err("must reject");
        assert!(matches!(err, InboxStoreError::InvalidId(_)));
        assert!(s.list().unwrap().is_empty());
    }

    use crate::domain::inbox_events::{Thread, fold_requests};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    fn comment(body: &str) -> InboxEventKind {
        InboxEventKind::Comment {
            thread: Thread::Overall,
            body: body.into(),
            warnings: vec![],
        }
    }

    fn mode(p: &Path) -> u32 {
        fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    // TS-05 / TS-24 (store level): an operator override is sticky.
    #[test]
    fn agent_priority_write_is_ignored_under_operator_override() {
        let s = store();
        let d = s.create("T").unwrap();
        s.set_priority(&d.id, PriorityWrite::Operator(Some(Priority::P0)))
            .unwrap()
            .expect("operator change");
        let none = s
            .set_priority(&d.id, PriorityWrite::Agent(Priority::P3))
            .unwrap();
        assert!(none.is_none());
        let cur = parsed(s.get(&d.id).unwrap());
        assert_eq!(cur.frontmatter.priority, Priority::P0);
        assert_eq!(cur.frontmatter.priority_source, PrioritySource::Operator);
    }

    // TS-64 / TS-06 (store level): every change is an event with from/to/source;
    // clearing hands control back to the agent.
    #[test]
    fn priority_changes_append_events_and_clear_returns_control() {
        let s = store();
        let d = s.create("T").unwrap();
        let e1 = s
            .set_priority(&d.id, PriorityWrite::Agent(Priority::P1))
            .unwrap()
            .unwrap();
        assert_eq!(
            e1.kind,
            InboxEventKind::PriorityChanged {
                from: Priority::P2,
                to: Priority::P1,
                source: PrioritySource::Agent
            }
        );
        assert_eq!(e1.author, AuthorKind::SystemAgent);
        s.set_priority(&d.id, PriorityWrite::Operator(Some(Priority::P0)))
            .unwrap()
            .unwrap();
        // Same value again is not a change.
        assert!(
            s.set_priority(&d.id, PriorityWrite::Operator(Some(Priority::P0)))
                .unwrap()
                .is_none()
        );
        let cleared = s
            .set_priority(&d.id, PriorityWrite::Operator(None))
            .unwrap();
        assert!(
            cleared.is_none(),
            "clearing keeps the value, only the source changes"
        );
        let cur = parsed(s.get(&d.id).unwrap());
        assert_eq!(cur.frontmatter.priority_source, PrioritySource::Agent);
        s.set_priority(&d.id, PriorityWrite::Agent(Priority::P3))
            .unwrap()
            .unwrap();
        let evs = s.read_events(&d.id).unwrap();
        let changes: Vec<_> = evs
            .iter()
            .filter_map(|e| match &e.kind {
                InboxEventKind::PriorityChanged { from, to, source } => Some((*from, *to, *source)),
                _ => None,
            })
            .collect();
        assert_eq!(
            changes,
            vec![
                (Priority::P2, Priority::P1, PrioritySource::Agent),
                (Priority::P1, Priority::P0, PrioritySource::Operator),
                (Priority::P0, Priority::P3, PrioritySource::Agent),
            ]
        );
    }

    // TS-08: a concurrent editor save and priority write never lose each other.
    #[test]
    fn concurrent_body_save_and_priority_write_both_survive() {
        for _ in 0..20 {
            let s = Arc::new(store());
            let d = s.create("T").unwrap();
            let hash = FileRevision::of_body(&d.body).body_hash;
            let (a, b) = (s.clone(), s.clone());
            let id1 = d.id.clone();
            let id2 = d.id.clone();
            let t1 = std::thread::spawn(move || a.save_body(&id1, &hash, "edited\n").unwrap());
            let t2 = std::thread::spawn(move || {
                b.set_priority(&id2, PriorityWrite::Operator(Some(Priority::P0)))
                    .unwrap()
            });
            t1.join().unwrap();
            t2.join().unwrap();
            let cur = parsed(s.get(&d.id).unwrap());
            assert_eq!(cur.body, "edited\n");
            assert_eq!(cur.frontmatter.priority, Priority::P0);
        }
    }

    // TS-10: 50 concurrent appends give 50 intact lines; a torn tail is tolerated.
    #[test]
    fn concurrent_appends_stay_line_intact_and_torn_tail_is_skipped() {
        let s = Arc::new(store());
        let d = s.create("T").unwrap();
        let handles: Vec<_> = (0..50)
            .map(|i| {
                let s = s.clone();
                let id = d.id.clone();
                std::thread::spawn(move || {
                    s.append_event(
                        &id,
                        EventAuthor::operator(),
                        None,
                        comment(&format!("c{i} {}", "x".repeat(2000))),
                    )
                    .unwrap()
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let path = s.dir().join(format!("{}.events.jsonl", d.id));
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 50);
        assert_eq!(s.read_events(&d.id).unwrap().len(), 50);
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        std::io::Write::write_all(&mut f, b"{\"schema_version\":1,\"event_id\":\"torn").unwrap();
        assert_eq!(s.read_events(&d.id).unwrap().len(), 50);
        // Appending after a torn tail still yields a readable line.
        s.append_event(&d.id, EventAuthor::operator(), None, comment("after"))
            .unwrap();
        assert_eq!(s.read_events(&d.id).unwrap().len(), 51);
    }

    #[test]
    fn append_requires_an_existing_draft_and_ids_are_server_issued() {
        let s = store();
        assert!(matches!(
            s.append_event(
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                EventAuthor::operator(),
                None,
                comment("x")
            ),
            Err(InboxStoreError::NotFound(_))
        ));
        let d = s.create("T").unwrap();
        let a = s
            .append_event(&d.id, EventAuthor::worktree_agent(), None, comment("a"))
            .unwrap();
        let b = s
            .append_event(
                &d.id,
                EventAuthor::operator(),
                Some(a.event_id.clone()),
                comment("b"),
            )
            .unwrap();
        assert!(is_ulid(&a.event_id) && a.event_id != b.event_id);
        assert_eq!(a.caller.as_deref(), Some("worktree"));
        assert_eq!(b.parent_event_id.as_deref(), Some(a.event_id.as_str()));
        assert!(fold_requests(&s.read_events(&d.id).unwrap()).is_empty());
    }

    // TS-39: draft, events and session files are 0600.
    #[test]
    fn store_files_are_owner_only() {
        let s = store();
        let d = s.create("T").unwrap();
        s.append_event(&d.id, EventAuthor::operator(), None, comment("c"))
            .unwrap();
        s.write_session(
            &d.id,
            &AgentSession {
                agent: "claude".into(),
                model: None,
                session_id: "s1".into(),
                turns: 0,
            },
        )
        .unwrap();
        for suffix in ["md", "events.jsonl", "session.json"] {
            let p = s.dir().join(format!("{}.{suffix}", d.id));
            assert_eq!(mode(&p), 0o600, "{suffix}");
        }
        assert_eq!(
            s.read_session(&d.id).unwrap().map(|x| x.session_id),
            Some("s1".to_string())
        );
    }

    // TS-53 (store level): v1 is upgraded on write; a newer version is refused.
    #[test]
    fn v1_is_upgraded_on_write_and_newer_versions_are_refused() {
        let s = store();
        let v1 = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let src = "---\nschema_version: 1\ntitle: Old\nstatus: Draft\ncreated_at: a\nupdated_at: a\nconversions: []\n---\nbody\n";
        fs::write(s.dir().join(format!("{v1}.md")), src).unwrap();
        s.set_priority(v1, PriorityWrite::Operator(Some(Priority::P1)))
            .unwrap();
        let cur = parsed(s.get(v1).unwrap());
        assert_eq!(cur.frontmatter.schema_version, INBOX_DRAFT_SCHEMA_VERSION);
        assert_eq!(cur.body, "body\n");

        let v9 = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
        fs::write(
            s.dir().join(format!("{v9}.md")),
            src.replace("schema_version: 1", "schema_version: 9"),
        )
        .unwrap();
        let hash = FileRevision::of_body("body\n").body_hash;
        assert!(matches!(
            s.save_body(v9, &hash, "x"),
            Err(InboxStoreError::UnsupportedVersion { version: 9, .. })
        ));
        assert!(matches!(
            s.set_priority(v9, PriorityWrite::Agent(Priority::P0)),
            Err(InboxStoreError::UnsupportedVersion { .. })
        ));
        assert!(matches!(
            s.merge_frontmatter(v9, FrontmatterAuthor::Editor, FrontmatterPatch::default()),
            Err(InboxStoreError::UnsupportedVersion { .. })
        ));
        // Reading still works.
        assert_eq!(parsed(s.get(v9).unwrap()).frontmatter.schema_version, 9);
    }

    // TS-54: Drop keeps sidecars, Delete removes them, the sweep clears orphans.
    #[test]
    fn drop_keeps_sidecars_delete_removes_them_and_sweep_clears_orphans() {
        let s = store();
        let d = s.create("T").unwrap();
        s.append_event(&d.id, EventAuthor::operator(), None, comment("c"))
            .unwrap();
        s.write_session(
            &d.id,
            &AgentSession {
                agent: "claude".into(),
                model: None,
                session_id: "s".into(),
                turns: 1,
            },
        )
        .unwrap();
        s.merge_frontmatter(
            &d.id,
            FrontmatterAuthor::Editor,
            FrontmatterPatch {
                status: Some(DraftStatus::Dropped),
                ..FrontmatterPatch::default()
            },
        )
        .unwrap();
        let ev = s.dir().join(format!("{}.events.jsonl", d.id));
        let se = s.dir().join(format!("{}.session.json", d.id));
        assert!(ev.is_file() && se.is_file());
        s.delete(&d.id).unwrap();
        assert!(!ev.exists() && !se.exists());

        let orphan = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        fs::write(s.dir().join(format!("{orphan}.events.jsonl")), "").unwrap();
        fs::write(s.dir().join(format!("{orphan}.session.json")), "{}").unwrap();
        let keep = s.create("K").unwrap();
        s.append_event(&keep.id, EventAuthor::operator(), None, comment("k"))
            .unwrap();
        let removed = s.sweep_orphans().unwrap();
        assert_eq!(removed.len(), 2);
        assert!(s.dir().join(format!("{}.events.jsonl", keep.id)).is_file());
        assert_eq!(s.list().unwrap().len(), 1);
    }
}
