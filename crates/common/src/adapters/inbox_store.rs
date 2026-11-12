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
    InboxDraftView, NOTE_DRAFT_SCHEMA_VERSION, Priority, PrioritySource, ProjectRef,
    parse_inbox_file, render_inbox_file,
};
use crate::domain::note_codec::{
    InboxNote, MAX_TITLE_CHARS, NOTES_SCHEMA_VERSION, is_valid_type, list_hash, new_note_id,
    parse_notes, render_notes,
};
use crate::domain::solution::{DraftKind, FactoryOverrides, idempotency_key};
use crate::util::id::{is_ulid, random_ulid};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use thiserror::Error;

/// Files kept beside `<id>.md`.
const SIDECAR_SUFFIXES: [&str; 3] = ["events.jsonl", "session.json", "coordinator.jsonl"];

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
    /// A note edit raced another write. `current` is the note as it now is
    /// (`None` for a structural change), with the list hash to retry against.
    #[error("note conflict on {id}")]
    NoteConflict {
        id: String,
        current: Option<Box<InboxNote>>,
        list_hash: String,
    },
    #[error("draft {id} has no note {note_id}")]
    NoteNotFound { id: String, note_id: String },
    #[error("invalid note: {0}")]
    InvalidNote(String),
    /// A whole-body write would have removed notes the item has.
    #[error("draft {id}: the new body drops notes {dropped:?}")]
    NoteIdsDropped { id: String, dropped: Vec<String> },
    /// A solution document write raced another (expected-hash mismatch).
    #[error("document {name} of {id} changed")]
    DocConflict {
        id: String,
        name: String,
        current: Option<String>,
    },
    #[error("invalid solution document name {0:?}")]
    InvalidDocName(String),
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

/// One lock per path, shared by every `InboxStore` in the process, so a
/// priority write and an editor save on the same draft never interleave their
/// read-modify-write. External editors are still unlocked.
fn path_lock(path: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let mut map = LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    map.entry(path.to_path_buf()).or_default().clone()
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

    fn ensure_dir(&self) -> Result<(), InboxStoreError> {
        fs::create_dir_all(&self.dir)?;
        let _ = fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700));
        Ok(())
    }

    fn sidecar(&self, id: &str, suffix: &str) -> Result<PathBuf, InboxStoreError> {
        self.path_for(id)?;
        Ok(self.dir.join(format!("{id}.{suffix}")))
    }

    /// Hold the draft's lock for the duration of `f`.
    fn locked<T>(
        &self,
        id: &str,
        f: impl FnOnce() -> Result<T, InboxStoreError>,
    ) -> Result<T, InboxStoreError> {
        let lock = path_lock(&self.path_for(id)?);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        f()
    }

    fn write_owner_only(path: &Path, contents: &str) -> Result<(), InboxStoreError> {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(contents.as_bytes())?;
        Ok(())
    }

    fn atomic_write(&self, id: &str, contents: &str) -> Result<(), InboxStoreError> {
        self.ensure_dir()?;
        let final_path = self.path_for(id)?;
        let tmp_path = self.dir.join(format!(
            "{id}.{}.{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        Self::write_owner_only(&tmp_path, contents)?;
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

    /// Load a draft for rewriting: refuse a newer schema, and stamp an older one
    /// with the version its content needs. Every write stamps, so the v2 -> v3
    /// step must be lossless whatever triggers it: the old body is re-rendered
    /// through the note codec as its single default note, escaping any line
    /// that a v3 reader would otherwise take for a marker. v4 is stamped only
    /// when a v4 field is in use, and a version is never lowered.
    fn require_writable(&self, id: &str) -> Result<InboxDraft, InboxStoreError> {
        let mut draft = self.require_parsed(id)?;
        let version = draft.frontmatter.schema_version;
        if version > INBOX_DRAFT_SCHEMA_VERSION {
            return Err(InboxStoreError::UnsupportedVersion {
                id: id.to_string(),
                version,
            });
        }
        if version < NOTES_SCHEMA_VERSION {
            draft.body = render_notes(&parse_notes(&draft.body, version));
        }
        stamp_version(&mut draft.frontmatter);
        Ok(draft)
    }

    /// The item's notes, read as its schema version wrote them, with the list
    /// hash that gates structural changes.
    pub fn notes(&self, id: &str) -> Result<(Vec<InboxNote>, String), InboxStoreError> {
        let draft = self.require_parsed(id)?;
        let notes = parse_notes(&draft.body, draft.frontmatter.schema_version);
        let lh = list_hash(&notes);
        Ok((notes, lh))
    }

    /// Rewrite the notes of `id` under its lock: `f` gets the current notes
    /// and returns what to keep. The result is rendered back as the body.
    fn with_notes<T>(
        &self,
        id: &str,
        f: impl FnOnce(&mut Vec<InboxNote>) -> Result<T, InboxStoreError>,
    ) -> Result<(T, Vec<InboxNote>), InboxStoreError> {
        self.locked(id, || {
            let mut draft = self.require_writable(id)?;
            let mut notes = parse_notes(&draft.body, NOTES_SCHEMA_VERSION);
            let out = f(&mut notes)?;
            if notes.is_empty() {
                notes.push(InboxNote::default_with(""));
            }
            draft.body = render_notes(&notes);
            draft.frontmatter.updated_at = now_rfc3339();
            self.atomic_write(id, &render_inbox_file(&draft))?;
            Ok((out, notes))
        })
    }

    /// Add a note after `after_id` (or last), if `expected_list_hash` is
    /// current. Returns the new note and the full list.
    pub fn add_note(
        &self,
        id: &str,
        expected_list_hash: &str,
        note_type: &str,
        title: &str,
        body: &str,
        after_id: Option<&str>,
    ) -> Result<(InboxNote, Vec<InboxNote>), InboxStoreError> {
        let (note_type, title) = validate_note(note_type, title)?;
        self.with_notes(id, |notes| {
            check_list(id, notes, expected_list_hash)?;
            let at = match after_id {
                None => notes.len(),
                Some(after) => {
                    notes.iter().position(|n| n.id == after).ok_or_else(|| {
                        InboxStoreError::NoteNotFound {
                            id: id.to_string(),
                            note_id: after.to_string(),
                        }
                    })? + 1
                }
            };
            let note = InboxNote {
                id: new_note_id(notes),
                note_type,
                title,
                body: body.replace("\r\n", "\n"),
            };
            notes.insert(at, note.clone());
            Ok(note)
        })
    }

    /// Edit one note, if `expected_hash` is that note's current hash. Other
    /// notes' edits never conflict with it.
    pub fn update_note(
        &self,
        id: &str,
        note_id: &str,
        expected_hash: &str,
        patch: NotePatch,
    ) -> Result<(InboxNote, Vec<InboxNote>), InboxStoreError> {
        self.with_notes(id, |notes| {
            let lh = list_hash(notes);
            let note = notes.iter_mut().find(|n| n.id == note_id).ok_or_else(|| {
                InboxStoreError::NoteNotFound {
                    id: id.to_string(),
                    note_id: note_id.to_string(),
                }
            })?;
            if note.hash() != expected_hash {
                return Err(InboxStoreError::NoteConflict {
                    id: id.to_string(),
                    current: Some(Box::new(note.clone())),
                    list_hash: lh,
                });
            }
            let (note_type, title) = validate_note(
                patch.note_type.as_deref().unwrap_or(&note.note_type),
                patch.title.as_deref().unwrap_or(&note.title),
            )?;
            note.note_type = note_type;
            note.title = title;
            if let Some(body) = patch.body {
                note.body = body.replace("\r\n", "\n");
            }
            Ok(note.clone())
        })
    }

    /// Remove one note, if `expected_list_hash` is current. Removing the last
    /// note leaves one empty default note: an item never has zero notes.
    pub fn delete_note(
        &self,
        id: &str,
        note_id: &str,
        expected_list_hash: &str,
    ) -> Result<Vec<InboxNote>, InboxStoreError> {
        self.with_notes(id, |notes| {
            check_list(id, notes, expected_list_hash)?;
            let at = notes.iter().position(|n| n.id == note_id).ok_or_else(|| {
                InboxStoreError::NoteNotFound {
                    id: id.to_string(),
                    note_id: note_id.to_string(),
                }
            })?;
            notes.remove(at);
            Ok(())
        })
        .map(|(_, notes)| notes)
    }

    /// Put the notes in `order`, which must name every note exactly once, if
    /// `expected_list_hash` is current. Ids are unchanged.
    pub fn reorder_notes(
        &self,
        id: &str,
        order: &[String],
        expected_list_hash: &str,
    ) -> Result<Vec<InboxNote>, InboxStoreError> {
        self.with_notes(id, |notes| {
            check_list(id, notes, expected_list_hash)?;
            let mut want: Vec<&String> = order.iter().collect();
            let mut have: Vec<&String> = notes.iter().map(|n| &n.id).collect();
            want.sort();
            have.sort();
            if want != have {
                return Err(InboxStoreError::InvalidNote(
                    "the order must name every note exactly once".to_string(),
                ));
            }
            let mut reordered = Vec::with_capacity(notes.len());
            for nid in order {
                if let Some(at) = notes.iter().position(|n| &n.id == nid) {
                    reordered.push(notes.remove(at));
                }
            }
            *notes = reordered;
            Ok(())
        })
        .map(|(_, notes)| notes)
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
        self.create_kind(title, DraftKind::Note)
    }

    /// Create an empty draft of `kind`. A note is written as v3 with no
    /// `kind` key; a solution item is v4.
    pub fn create_kind(&self, title: &str, kind: DraftKind) -> Result<InboxDraft, InboxStoreError> {
        let id = random_ulid();
        let now = now_rfc3339();
        let mut draft = InboxDraft {
            id: id.clone(),
            frontmatter: InboxDraftFrontmatter {
                schema_version: NOTE_DRAFT_SCHEMA_VERSION,
                title: title.to_string(),
                project: None,
                status: DraftStatus::Draft,
                created_at: now.clone(),
                updated_at: now,
                conversions: Vec::new(),
                priority: Priority::default(),
                priority_source: PrioritySource::default(),
                kind: (kind == DraftKind::Solution).then_some(kind),
                factory: None,
                extra: serde_yaml::Mapping::new(),
            },
            body: String::new(),
        };
        stamp_version(&mut draft.frontmatter);
        self.atomic_write(&id, &render_inbox_file(&draft))?;
        Ok(draft)
    }

    /// Replace the item's `factory:` overrides; an empty set removes the key.
    /// Setting overrides on a note stamps it v4.
    pub fn set_factory_overrides(
        &self,
        id: &str,
        overrides: FactoryOverrides,
    ) -> Result<InboxDraft, InboxStoreError> {
        self.locked(id, || {
            let mut draft = self.require_writable(id)?;
            draft.frontmatter.factory = (!overrides.is_empty()).then_some(overrides);
            stamp_version(&mut draft.frontmatter);
            draft.frontmatter.updated_at = now_rfc3339();
            self.atomic_write(id, &render_inbox_file(&draft))?;
            Ok(draft)
        })
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
        self.locked(id, || {
            // The token is the hash of the body the client read, before any
            // v3 re-rendering this write might do.
            let on_disk = self.require_parsed(id)?.body;
            let mut draft = self.require_writable(id)?;
            let actual = FileRevision::of_body(&on_disk);
            if actual.body_hash != expected_hash {
                return Err(InboxStoreError::Conflict {
                    id: id.to_string(),
                    expected: expected_hash.to_string(),
                    actual: actual.body_hash,
                });
            }
            let before = parse_notes(&draft.body, NOTES_SCHEMA_VERSION);
            let after = parse_notes(body, NOTES_SCHEMA_VERSION);
            let dropped: Vec<String> = before
                .iter()
                .filter(|b| !after.iter().any(|a| a.id == b.id))
                .map(|b| b.id.clone())
                .collect();
            if !dropped.is_empty() {
                return Err(InboxStoreError::NoteIdsDropped {
                    id: id.to_string(),
                    dropped,
                });
            }
            draft.body = render_notes(&after);
            draft.frontmatter.updated_at = now_rfc3339();
            self.atomic_write(id, &render_inbox_file(&draft))?;
            Ok(draft)
        })
    }

    /// Rewrite the body without a hash check. Frontmatter (including
    /// `conversions[]`) is re-read and written back unchanged except `updated_at`.
    pub fn force_write_body(&self, id: &str, body: &str) -> Result<InboxDraft, InboxStoreError> {
        self.locked(id, || {
            let mut draft = self.require_writable(id)?;
            draft.body = body.to_string();
            draft.frontmatter.updated_at = now_rfc3339();
            self.atomic_write(id, &render_inbox_file(&draft))?;
            Ok(draft)
        })
    }

    /// Remove the draft file. Unparseable drafts delete like any other — a
    /// malformed file must not become undeletable.
    pub fn delete(&self, id: &str) -> Result<(), InboxStoreError> {
        let path = self.path_for(id)?;
        if !path.exists() {
            return Err(InboxStoreError::NotFound(id.to_string()));
        }
        // Sidecars first: a crash between the two leaves an orphan the startup
        // sweep removes, never a draft whose history has vanished.
        match fs::remove_dir_all(self.dir.join(id)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        for suffix in SIDECAR_SUFFIXES {
            match fs::remove_file(self.sidecar(id, suffix)?) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
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
        self.locked(id, || {
            let mut draft = self.require_writable(id)?;
            apply_patch(&mut draft.frontmatter, author, patch);
            draft.frontmatter.updated_at = now_rfc3339();
            self.atomic_write(id, &render_inbox_file(&draft))?;
            Ok(draft)
        })
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
        self.set_priority_by(id, write, None)
    }

    /// As [`Self::set_priority`], stamping the event with the self-declared
    /// `caller` marker. The marker is unauthenticated (T-01): it records what
    /// the caller said it was, not what it is.
    pub fn set_priority_by(
        &self,
        id: &str,
        write: PriorityWrite,
        caller: Option<String>,
    ) -> Result<Option<InboxEvent>, InboxStoreError> {
        self.locked(id, || {
            let mut draft = self.require_writable(id)?;
            let fm = &mut draft.frontmatter;
            let from = fm.priority;
            let (to, source, author) = match write {
                PriorityWrite::Operator(Some(p)) => {
                    (p, PrioritySource::Operator, EventAuthor::operator())
                }
                PriorityWrite::Operator(None) => {
                    (from, PrioritySource::Agent, EventAuthor::operator())
                }
                PriorityWrite::Agent(p) => {
                    if fm.priority_source == PrioritySource::Operator {
                        return Ok(None);
                    }
                    (p, PrioritySource::Agent, EventAuthor::system_agent())
                }
            };
            if to == from && source == fm.priority_source {
                return Ok(None);
            }
            fm.priority = to;
            fm.priority_source = source;
            fm.updated_at = now_rfc3339();
            self.atomic_write(id, &render_inbox_file(&draft))?;
            if to == from {
                return Ok(None);
            }
            let author = EventAuthor {
                caller: caller.or(author.caller),
                ..author
            };
            let event = self.append_event(
                id,
                author,
                None,
                InboxEventKind::PriorityChanged { from, to, source },
            )?;
            Ok(Some(event))
        })
    }

    /// Append one event to `<id>.events.jsonl`. The draft must exist.
    pub fn append_event(
        &self,
        id: &str,
        author: EventAuthor,
        parent_event_id: Option<String>,
        kind: InboxEventKind,
    ) -> Result<InboxEvent, InboxStoreError> {
        if !self.path_for(id)?.is_file() {
            return Err(InboxStoreError::NotFound(id.to_string()));
        }
        let path = self.sidecar(id, "events.jsonl")?;
        // One lock per log keeps appends single-writer within the process;
        // O_APPEND plus a single write keeps each line whole.
        let lock = path_lock(&path);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        self.append_unlocked(&path, author, parent_event_id, kind)
    }

    /// Append `kind` unless an earlier event has the same
    /// [`idempotency_key`]. Returns the event and whether it was appended: a
    /// repeat returns the existing event and writes nothing. Check and append
    /// happen under the log's lock, so two racing writers record once.
    pub fn append_event_once(
        &self,
        id: &str,
        author: EventAuthor,
        parent_event_id: Option<String>,
        kind: InboxEventKind,
    ) -> Result<(InboxEvent, bool), InboxStoreError> {
        if !self.path_for(id)?.is_file() {
            return Err(InboxStoreError::NotFound(id.to_string()));
        }
        let path = self.sidecar(id, "events.jsonl")?;
        let lock = path_lock(&path);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(key) = idempotency_key(&kind) {
            let existing = self
                .read_events(id)?
                .into_iter()
                .find(|e| idempotency_key(&e.kind).as_deref() == Some(key.as_str()));
            if let Some(e) = existing {
                return Ok((e, false));
            }
        }
        Ok((
            self.append_unlocked(&path, author, parent_event_id, kind)?,
            true,
        ))
    }

    fn append_unlocked(
        &self,
        path: &Path,
        author: EventAuthor,
        parent_event_id: Option<String>,
        kind: InboxEventKind,
    ) -> Result<InboxEvent, InboxStoreError> {
        let path = path.to_path_buf();
        let event = InboxEvent {
            schema_version: INBOX_EVENT_SCHEMA_VERSION,
            event_id: random_ulid(),
            ts: now_rfc3339(),
            author: author.kind,
            caller: author.caller,
            parent_event_id,
            kind,
        };
        let mut line = serde_json::to_string(&event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        line.push('\n');
        self.ensure_dir()?;
        let mut f = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(&path)?;
        // A torn tail (crash mid-write) would swallow this line; start a fresh one.
        if !ends_with_newline(&path)? {
            line.insert(0, '\n');
        }
        f.write_all(line.as_bytes())?;
        Ok(event)
    }

    /// Every parseable event, in append order. A torn or foreign line is skipped.
    pub fn read_events(&self, id: &str) -> Result<Vec<InboxEvent>, InboxStoreError> {
        let path = self.sidecar(id, "events.jsonl")?;
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        Ok(text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<InboxEvent>(l).ok())
            .collect())
    }

    pub fn read_session(&self, id: &str) -> Result<Option<AgentSession>, InboxStoreError> {
        let path = self.sidecar(id, "session.json")?;
        match fs::read_to_string(&path) {
            Ok(t) => Ok(serde_json::from_str(&t).ok()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn write_session(&self, id: &str, session: &AgentSession) -> Result<(), InboxStoreError> {
        let path = self.sidecar(id, "session.json")?;
        self.ensure_dir()?;
        let json = serde_json::to_string_pretty(session)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = self
            .dir
            .join(format!("{id}.session.{}.tmp", std::process::id()));
        Self::write_owner_only(&tmp, &json)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// `<dir>/<id>/solution`, created 0700 on demand.
    fn solution_dir(&self, id: &str) -> Result<PathBuf, InboxStoreError> {
        self.path_for(id)?;
        Ok(self.dir.join(id).join("solution"))
    }

    /// The item's solution documents and their SHA-256s, by name
    /// (`handover/<slug>.md` included).
    pub fn solution_docs(
        &self,
        id: &str,
    ) -> Result<std::collections::BTreeMap<String, String>, InboxStoreError> {
        let dir = self.solution_dir(id)?;
        let mut out = std::collections::BTreeMap::new();
        for sub in ["", "handover/"] {
            let Ok(entries) = fs::read_dir(dir.join(sub)) else {
                continue;
            };
            for e in entries.flatten() {
                let name = format!("{sub}{}", e.file_name().to_string_lossy());
                if name.ends_with(".md") && e.path().is_file() {
                    let text = fs::read_to_string(e.path())?;
                    out.insert(name, crate::domain::solution::sha256_hex(&text));
                }
            }
        }
        Ok(out)
    }

    /// One solution document and its SHA-256, or `None`.
    pub fn read_solution_doc(
        &self,
        id: &str,
        name: &str,
    ) -> Result<Option<(String, String)>, InboxStoreError> {
        check_doc_name(name)?;
        match fs::read_to_string(self.solution_dir(id)?.join(name)) {
            Ok(text) => {
                let h = crate::domain::solution::sha256_hex(&text);
                Ok(Some((text, h)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Write a solution document if its current SHA-256 is `expected`
    /// (`None` = must not exist yet, `Some("*")` = any). Temp-then-rename,
    /// 0600 in a 0700 tree, under the item lock. Returns the new hash.
    pub fn write_solution_doc(
        &self,
        id: &str,
        name: &str,
        content: &str,
        expected: Option<&str>,
    ) -> Result<String, InboxStoreError> {
        check_doc_name(name)?;
        self.require_parsed(id)?;
        self.locked(id, || {
            let dir = self.solution_dir(id)?;
            let path = dir.join(name);
            let current = fs::read_to_string(&path)
                .ok()
                .map(|t| crate::domain::solution::sha256_hex(&t));
            let ok = match expected {
                Some("*") => true,
                Some(h) => current.as_deref() == Some(h),
                None => current.is_none(),
            };
            if !ok {
                return Err(InboxStoreError::DocConflict {
                    id: id.to_string(),
                    name: name.to_string(),
                    current,
                });
            }
            let parent = path.parent().unwrap_or(&dir).to_path_buf();
            fs::create_dir_all(&parent)?;
            for d in [self.dir.join(id), dir.clone(), parent.clone()] {
                let _ = fs::set_permissions(&d, fs::Permissions::from_mode(0o700));
            }
            let tmp = parent.join(format!(".{}.tmp", random_ulid()));
            Self::write_owner_only(&tmp, content)?;
            fs::rename(&tmp, &path)?;
            Ok(crate::domain::solution::sha256_hex(content))
        })
    }

    /// Append one line to `<id>.coordinator.jsonl` (0600, append-only).
    pub fn append_transcript(
        &self,
        id: &str,
        entry: &serde_json::Value,
    ) -> Result<(), InboxStoreError> {
        self.require_parsed(id)?;
        let path = self.sidecar(id, "coordinator.jsonl")?;
        let lock = path_lock(&path);
        let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_dir()?;
        let mut line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        line.push('\n');
        let mut f = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(&path)?;
        f.write_all(line.as_bytes())?;
        Ok(())
    }

    /// Every transcript line, in order; a torn line is skipped.
    pub fn read_transcript(&self, id: &str) -> Result<Vec<serde_json::Value>, InboxStoreError> {
        let path = self.sidecar(id, "coordinator.jsonl")?;
        match fs::read_to_string(&path) {
            Ok(t) => Ok(t
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// Remove sidecars whose draft no longer exists. Returns what was removed.
    pub fn sweep_orphans(&self) -> Result<Vec<PathBuf>, InboxStoreError> {
        let mut removed = Vec::new();
        let entries = match fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(removed),
            Err(e) => return Err(e.into()),
        };
        for ent in entries.flatten() {
            let name = ent.file_name();
            let name = name.to_string_lossy();
            let Some(id) = SIDECAR_SUFFIXES
                .iter()
                .find_map(|sfx| name.strip_suffix(&format!(".{sfx}")))
            else {
                continue;
            };
            if is_ulid(id) && !self.dir.join(format!("{id}.md")).exists() {
                fs::remove_file(ent.path())?;
                removed.push(ent.path());
            }
        }
        Ok(removed)
    }
}

/// A solution document name: `<name>.md` or `handover/<slug>.md`, lowercase
/// letters, digits and dashes only, so it cannot leave `solution/`.
pub fn check_doc_name(name: &str) -> Result<(), InboxStoreError> {
    let ok_stem = |s: &str| {
        !s.is_empty()
            && s.len() <= 64
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    let stem = name.strip_prefix("handover/").unwrap_or(name);
    match stem.strip_suffix(".md") {
        Some(s) if ok_stem(s) => Ok(()),
        _ => Err(InboxStoreError::InvalidDocName(name.to_string())),
    }
}

/// Stamp the lowest version that holds the frontmatter, never lowering it.
fn stamp_version(fm: &mut InboxDraftFrontmatter) {
    fm.schema_version = fm
        .schema_version
        .max(NOTES_SCHEMA_VERSION)
        .max(fm.required_schema_version());
}

fn ends_with_newline(path: &Path) -> Result<bool, InboxStoreError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(path)?;
    if f.seek(SeekFrom::End(-1)).is_err() {
        return Ok(true);
    }
    let mut last = [0u8; 1];
    f.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
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
            // Append-only: a record already present stays where it is, and
            // only new ones are added, so a conversion's index never moves
            // (the graph keys worktrees on it) and a re-conversion can never
            // erase history.
            if let Some(conversions) = patch.conversions {
                for entry in conversions {
                    if !fm.conversions.contains(&entry) {
                        fm.conversions.push(entry);
                    }
                }
            }
            if patch.status == Some(DraftStatus::Promoted) {
                fm.status = DraftStatus::Promoted;
            }
        }
    }
}

/// A partial note edit: absent fields keep their value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotePatch {
    pub note_type: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
}

/// A trimmed title and a valid type, or why not.
fn validate_note(note_type: &str, title: &str) -> Result<(String, String), InboxStoreError> {
    if !is_valid_type(note_type) {
        return Err(InboxStoreError::InvalidNote(format!(
            "type must be a slug of [a-z0-9_]{{1,40}}, got {note_type:?}"
        )));
    }
    let title = title.trim().replace(['\n', '\r'], " ");
    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
        return Err(InboxStoreError::InvalidNote(format!(
            "a title needs 1 to {MAX_TITLE_CHARS} characters"
        )));
    }
    Ok((note_type.to_string(), title))
}

/// Refuse a structural change made against a stale list.
fn check_list(id: &str, notes: &[InboxNote], expected: &str) -> Result<(), InboxStoreError> {
    let current = list_hash(notes);
    if current == expected {
        Ok(())
    } else {
        Err(InboxStoreError::NoteConflict {
            id: id.to_string(),
            current: None,
            list_hash: current,
        })
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

    // TS-73: a conversion patch appends; it never replaces earlier records, so
    // the conversion index stays a stable graph key across re-conversion.
    #[test]
    fn conversion_patches_append_and_never_replace() {
        let s = store();
        let d = s.create("Idea").unwrap();
        let v = |x: &str| serde_yaml::Value::String(x.to_string());
        for patch in [vec![v("a")], vec![v("b")], vec![v("a"), v("b"), v("c")]] {
            s.merge_frontmatter(
                &d.id,
                FrontmatterAuthor::Job,
                FrontmatterPatch {
                    conversions: Some(patch),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let fm = parsed(s.get(&d.id).unwrap()).frontmatter;
        assert_eq!(fm.conversions, vec![v("a"), v("b"), v("c")]);
    }

    // --- Multiple notes (graph/notes track phase 1) -------------------------

    use crate::domain::note_codec::{InboxNote, list_hash};

    fn with_notes(s: &InboxStore) -> (String, Vec<InboxNote>) {
        let d = s.create("Idea").unwrap();
        let (_, lh) = s.notes(&d.id).unwrap();
        let (_, notes) = s
            .add_note(
                &d.id,
                &lh,
                "acceptance_criteria",
                "Acceptance Criteria",
                "- works",
                None,
            )
            .unwrap();
        assert_eq!(notes.len(), 2);
        let (notes, _) = s.notes(&d.id).unwrap();
        (d.id, notes)
    }

    // TS-17 (store): a note is added after a given note and listed in order.
    #[test]
    fn notes_are_added_in_order_and_stamp_v3() {
        let s = store();
        let (id, notes) = with_notes(&s);
        assert_eq!(notes[0].id, "n1");
        assert_eq!(notes[1].note_type, "acceptance_criteria");
        let lh = list_hash(&notes);
        let (added, notes) = s
            .add_note(&id, &lh, "context", "Context", "ctx", Some("n1"))
            .unwrap();
        assert_eq!(notes[1].id, added.id);
        assert_eq!(parsed(s.get(&id).unwrap()).frontmatter.schema_version, 3);
    }

    // TS-18 (store): a bad type or empty title is refused, file unchanged.
    #[test]
    fn an_invalid_note_is_refused() {
        let s = store();
        let (id, notes) = with_notes(&s);
        let before = fs::read_to_string(s.dir().join(format!("{id}.md"))).unwrap();
        let lh = list_hash(&notes);
        for (t, title) in [("Bad Type", "T"), ("general", "  ")] {
            assert!(matches!(
                s.add_note(&id, &lh, t, title, "b", None),
                Err(InboxStoreError::InvalidNote(_))
            ));
        }
        assert_eq!(
            fs::read_to_string(s.dir().join(format!("{id}.md"))).unwrap(),
            before
        );
    }

    // TS-20: a stale note hash conflicts with the current note; another note's
    // edit with its own hash succeeds.
    #[test]
    fn note_edits_conflict_per_note_only() {
        let s = store();
        let (id, notes) = with_notes(&s);
        let (a, b) = (notes[0].clone(), notes[1].clone());
        s.update_note(
            &id,
            &a.id,
            &a.hash(),
            NotePatch {
                body: Some("A1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        match s.update_note(
            &id,
            &a.id,
            &a.hash(),
            NotePatch {
                body: Some("A2".into()),
                ..Default::default()
            },
        ) {
            Err(InboxStoreError::NoteConflict { current, .. }) => {
                assert_eq!(current.expect("current").body, "A1")
            }
            other => panic!("expected conflict, got {other:?}"),
        }
        let (edited, _) = s
            .update_note(
                &id,
                &b.id,
                &b.hash(),
                NotePatch {
                    title: Some("AC".into()),
                    ..Default::default()
                },
            )
            .expect("other note edits cleanly");
        assert_eq!(edited.title, "AC");
    }

    // TS-22 / TS-57 (store): structural changes need the current list hash.
    #[test]
    fn structural_changes_need_the_current_list_hash() {
        let s = store();
        let (id, notes) = with_notes(&s);
        let stale = list_hash(&notes);
        s.update_note(
            &id,
            "n1",
            &notes[0].hash(),
            NotePatch {
                body: Some("x".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(matches!(
            s.add_note(&id, &stale, "general", "T", "b", None),
            Err(InboxStoreError::NoteConflict { .. })
        ));
        assert!(matches!(
            s.reorder_notes(&id, &[notes[1].id.clone(), "n1".into()], &stale),
            Err(InboxStoreError::NoteConflict { .. })
        ));
        assert!(matches!(
            s.delete_note(&id, "n1", &stale),
            Err(InboxStoreError::NoteConflict { .. })
        ));
    }

    // TS-23 / TS-24 (store): reorder keeps ids; a non-permutation is refused.
    #[test]
    fn reorder_keeps_ids_and_needs_a_permutation() {
        let s = store();
        let (id, notes) = with_notes(&s);
        let lh = list_hash(&notes);
        assert!(matches!(
            s.reorder_notes(&id, &["n1".into()], &lh),
            Err(InboxStoreError::InvalidNote(_))
        ));
        let order = vec![notes[1].id.clone(), "n1".into()];
        let after = s.reorder_notes(&id, &order, &lh).unwrap();
        assert_eq!(
            after.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
            order
        );
        assert_eq!(after[1].body, notes[0].body);
    }

    // TS-25 / TS-26 (store): delete removes a note; the last one is cleared.
    #[test]
    fn deleting_the_last_note_clears_it() {
        let s = store();
        let (id, notes) = with_notes(&s);
        let after = s
            .delete_note(&id, &notes[1].id, &list_hash(&notes))
            .unwrap();
        assert_eq!(after.len(), 1);
        let after = s.delete_note(&id, "n1", &list_hash(&after)).unwrap();
        assert_eq!(after, vec![InboxNote::default_with("")]);
        assert!(matches!(
            s.delete_note(&id, "zz99", &list_hash(&after)),
            Err(InboxStoreError::NoteNotFound { .. })
        ));
    }

    // TS-29: the first note write on a legacy item keeps its text as n1.
    #[test]
    fn a_legacy_item_keeps_its_text_as_the_first_note() {
        let s = store();
        let d = s.create("Old").unwrap();
        let hash = FileRevision::of_body("").body_hash;
        s.save_body(&d.id, &hash, "Legacy text\n").unwrap();
        let (notes, lh) = s.notes(&d.id).unwrap();
        s.add_note(&d.id, &lh, "general", "More", "m", None)
            .unwrap();
        let (notes2, _) = s.notes(&d.id).unwrap();
        assert_eq!(notes2[0], notes[0]);
        assert_eq!(notes2[0].body, "Legacy text\n");
    }

    // TS-64: a v2 body with marker lookalikes survives a non-note write (the
    // v3 stamp) byte-identically as n1.
    #[test]
    fn a_v2_body_survives_a_priority_write_byte_identically() {
        let s = store();
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let body = "intro\n<!-- sebenza:note id=ab type=general title=\"Forged\" -->\n\\<!-- sebenza:note x\nend\n";
        let src = format!(
            "---\nschema_version: 2\ntitle: Old\nstatus: Draft\ncreated_at: a\nupdated_at: a\nconversions: []\n---\n{body}"
        );
        fs::write(s.dir().join(format!("{id}.md")), src).unwrap();
        s.set_priority(id, PriorityWrite::Agent(Priority::P1))
            .unwrap();
        let d = parsed(s.get(id).unwrap());
        assert_eq!(d.frontmatter.schema_version, 3);
        let (notes, _) = s.notes(id).unwrap();
        assert_eq!(notes, vec![InboxNote::default_with(body)]);
    }

    // TS-47: a legacy whole-body PUT that would drop a note id is refused.
    #[test]
    fn a_legacy_body_put_cannot_drop_notes() {
        let s = store();
        let (id, _) = with_notes(&s);
        let raw = parsed(s.get(&id).unwrap()).body;
        let hash = FileRevision::of_body(&raw).body_hash;
        assert!(matches!(
            s.save_body(&id, &hash, "flattened"),
            Err(InboxStoreError::NoteIdsDropped { .. })
        ));
        assert_eq!(parsed(s.get(&id).unwrap()).body, raw);
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
        // A note is upgraded to v3, not v4: v4 is stamped lazily.
        assert_eq!(cur.frontmatter.schema_version, NOTE_DRAFT_SCHEMA_VERSION);
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

    // TS-02 (store half): v4 is stamped lazily. A note stays v3 with no `kind`
    // key, so an older binary can still write it; a solution item, or a note
    // given factory overrides, is v4; a version is never lowered.
    #[test]
    fn v4_is_stamped_only_when_a_v4_field_is_used() {
        use crate::domain::solution::{AgentProfile, DraftKind, FactoryOverrides};
        let s = store();
        let note = s.create("Note").unwrap();
        assert_eq!(note.frontmatter.schema_version, NOTE_DRAFT_SCHEMA_VERSION);
        let text = fs::read_to_string(s.dir().join(format!("{}.md", note.id))).unwrap();
        assert!(
            !text.contains("kind:"),
            "a note writes no kind key:\n{text}"
        );
        assert!(!text.contains("factory:"));
        s.set_priority(&note.id, PriorityWrite::Operator(Some(Priority::P1)))
            .unwrap();
        let cur = parsed(s.get(&note.id).unwrap());
        assert_eq!(cur.frontmatter.schema_version, NOTE_DRAFT_SCHEMA_VERSION);
        assert_eq!(cur.frontmatter.kind(), DraftKind::Note);

        let sol = s.create_kind("Solution", DraftKind::Solution).unwrap();
        let cur = parsed(s.get(&sol.id).unwrap());
        assert_eq!(cur.frontmatter.schema_version, INBOX_DRAFT_SCHEMA_VERSION);
        assert_eq!(cur.frontmatter.kind(), DraftKind::Solution);
        // A later write keeps it a v4 solution item.
        s.set_priority(&sol.id, PriorityWrite::Operator(Some(Priority::P0)))
            .unwrap();
        let cur = parsed(s.get(&sol.id).unwrap());
        assert_eq!(cur.frontmatter.schema_version, INBOX_DRAFT_SCHEMA_VERSION);
        assert_eq!(cur.frontmatter.kind(), DraftKind::Solution);

        let overrides = FactoryOverrides {
            designer: Some(AgentProfile::new("claude", Some("opus"))),
            ..Default::default()
        };
        s.set_factory_overrides(&note.id, overrides.clone())
            .unwrap();
        let cur = parsed(s.get(&note.id).unwrap());
        assert_eq!(cur.frontmatter.schema_version, INBOX_DRAFT_SCHEMA_VERSION);
        assert_eq!(cur.frontmatter.factory, Some(overrides));
        assert_eq!(cur.frontmatter.kind(), DraftKind::Note);

        s.set_factory_overrides(&note.id, FactoryOverrides::default())
            .unwrap();
        let cur = parsed(s.get(&note.id).unwrap());
        assert_eq!(
            cur.frontmatter.factory, None,
            "an empty set removes the key"
        );
        assert_eq!(
            cur.frontmatter.schema_version, INBOX_DRAFT_SCHEMA_VERSION,
            "never lowered"
        );
    }

    // TS-02 (compat half): a v3 note with markers keeps its notes, and v4 keys
    // this binary reads survive a rewrite by an editor patch.
    #[test]
    fn a_v3_note_reads_and_writes_as_before() {
        let s = store();
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
        let src = "---\nschema_version: 3\ntitle: Old\nstatus: Draft\ncreated_at: a\nupdated_at: a\nconversions: []\n---\nbody\n";
        fs::write(s.dir().join(format!("{id}.md")), src).unwrap();
        let (before, _) = s.notes(id).unwrap();
        s.merge_frontmatter(
            id,
            FrontmatterAuthor::Editor,
            FrontmatterPatch {
                title: Some("Renamed".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let cur = parsed(s.get(id).unwrap());
        assert_eq!(cur.frontmatter.schema_version, NOTE_DRAFT_SCHEMA_VERSION);
        assert_eq!(cur.frontmatter.title, "Renamed");
        assert_eq!(s.notes(id).unwrap().0, before);
    }

    fn approval(h: &str) -> InboxEventKind {
        use crate::domain::solution::CredentialClass;
        InboxEventKind::DesignApproved {
            doc_hashes: std::collections::BTreeMap::from([("design.md".into(), h.into())]),
            credential: CredentialClass::OperatorSession,
        }
    }

    // FT-08: a keyed event is recorded once; a repeat returns the original.
    #[test]
    fn keyed_events_append_once() {
        let s = store();
        let id = s.create("T").unwrap().id;
        let (first, appended) = s
            .append_event_once(&id, EventAuthor::operator(), None, approval("h1"))
            .unwrap();
        assert!(appended);
        let (again, appended) = s
            .append_event_once(&id, EventAuthor::operator(), None, approval("h1"))
            .unwrap();
        assert!(!appended);
        assert_eq!(again.event_id, first.event_id);
        let (_, appended) = s
            .append_event_once(&id, EventAuthor::operator(), None, approval("h2"))
            .unwrap();
        assert!(appended, "a different hash set is a new approval");
        // Unkeyed events always append.
        for _ in 0..2 {
            let (_, appended) = s
                .append_event_once(&id, EventAuthor::operator(), None, comment("x"))
                .unwrap();
            assert!(appended);
        }
        assert_eq!(s.read_events(&id).unwrap().len(), 4);
        assert!(matches!(
            s.append_event_once(
                "01ARZ3NDEKTSV4RRFFQ69G5FZZ",
                EventAuthor::operator(),
                None,
                approval("h")
            ),
            Err(InboxStoreError::NotFound(_))
        ));
    }

    #[test]
    fn racing_keyed_appends_record_once() {
        let s = Arc::new(store());
        let id = s.create("T").unwrap().id;
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let s = s.clone();
                let id = id.clone();
                std::thread::spawn(move || {
                    s.append_event_once(&id, EventAuthor::operator(), None, approval("h"))
                        .unwrap()
                        .1
                })
            })
            .collect();
        let appended = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|a| *a)
            .count();
        assert_eq!(appended, 1);
        assert_eq!(s.read_events(&id).unwrap().len(), 1);
    }

    // TS-06 (documents): a stale expected hash is a conflict and nothing is
    // overwritten; names cannot escape solution/; delete removes the tree.
    #[test]
    fn solution_docs_are_hash_gated_and_confined() {
        let s = store();
        let id = s.create("S").unwrap().id;
        let h1 = s
            .write_solution_doc(&id, "requirements.md", "v1", None)
            .unwrap();
        assert!(matches!(
            s.write_solution_doc(&id, "requirements.md", "again", None),
            Err(InboxStoreError::DocConflict { .. })
        ));
        let h2 = s
            .write_solution_doc(&id, "requirements.md", "v2", Some(&h1))
            .unwrap();
        let err = s
            .write_solution_doc(&id, "requirements.md", "stale", Some(&h1))
            .unwrap_err();
        assert!(
            matches!(err, InboxStoreError::DocConflict { current: Some(ref c), .. } if *c == h2)
        );
        assert_eq!(
            s.read_solution_doc(&id, "requirements.md")
                .unwrap()
                .unwrap()
                .0,
            "v2"
        );
        s.write_solution_doc(&id, "handover/acme-billing.md", "do x", None)
            .unwrap();
        let docs = s.solution_docs(&id).unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs["requirements.md"], h2);
        for bad in [
            "../x.md",
            "handover/../../x.md",
            "x.txt",
            "UP.md",
            "a/b.md",
            "",
        ] {
            assert!(
                s.write_solution_doc(&id, bad, "x", Some("*")).is_err(),
                "{bad}"
            );
        }
        let doc = s.dir().join(&id).join("solution/requirements.md");
        assert_eq!(mode(&doc), 0o600);
        assert_eq!(mode(&s.dir().join(&id).join("solution")), 0o700);
        s.append_transcript(&id, &serde_json::json!({"kind": "operator", "text": "hi"}))
            .unwrap();
        assert_eq!(s.read_transcript(&id).unwrap().len(), 1);
        assert_eq!(
            mode(&s.dir().join(format!("{id}.coordinator.jsonl"))),
            0o600
        );
        s.delete(&id).unwrap();
        assert!(!s.dir().join(&id).exists());
        assert!(!s.dir().join(format!("{id}.coordinator.jsonl")).exists());
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
