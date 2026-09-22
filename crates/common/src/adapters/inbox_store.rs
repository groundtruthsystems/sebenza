//! Filesystem store for inbox drafts: `~/.ai/sebenza/inbox/<ulid>.md`.
//!
//! Writes are temp-file-then-rename. Body saves are gated on `body_hash`.
//! Frontmatter merges are key-scoped by author so a conversion job cannot
//! clobber the title and an editor cannot drop `conversions[]`.

use crate::domain::model::{
    DraftStatus, FileRevision, INBOX_DRAFT_SCHEMA_VERSION, InboxDraft, InboxDraftFrontmatter,
    InboxDraftView, ProjectRef, parse_inbox_file, render_inbox_file,
};
use crate::util::id::{is_ulid, random_ulid};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

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
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Who is writing frontmatter. Each author owns a disjoint key set, except
/// `Promoted` which the job sets and which wins a race with `Dropped`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontmatterAuthor {
    Editor,
    Job,
}

#[derive(Debug, Clone, Default)]
pub struct FrontmatterPatch {
    pub title: Option<String>,
    /// `Some(None)` unlinks the project.
    pub project: Option<Option<ProjectRef>>,
    pub status: Option<DraftStatus>,
    pub conversions: Option<Vec<serde_yaml::Value>>,
}

pub struct InboxStore {
    dir: PathBuf,
}

fn default_inbox_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".ai").join("sebenza").join("inbox")
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

impl InboxStore {
    pub fn new() -> Self {
        Self {
            dir: default_inbox_dir(),
        }
    }

    pub fn with_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

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
            },
            body: String::new(),
        };
        self.atomic_write(&id, &render_inbox_file(&draft))?;
        Ok(draft)
    }

    pub fn get(&self, id: &str) -> Result<InboxDraftView, InboxStoreError> {
        self.read_view(id)
    }

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
        let dir = std::env::temp_dir().join(format!(
            "sebenza-inbox-store-{}-{}",
            std::process::id(),
            n
        ));
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
        assert!(path.is_file(), "draft file should exist at {}", path.display());
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
        assert!(listed.iter().any(|v| matches!(v, InboxDraftView::Raw { .. })));
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
}
