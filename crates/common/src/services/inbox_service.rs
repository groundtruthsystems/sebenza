//! Inbox draft orchestration over [`InboxStore`] and the projects registry.
//!
//! The store owns bytes on disk; this layer owns the rules the dashboard and
//! `sebenza-cli` share: which drafts a listing shows, how a project link is
//! resolved (and what an unresolvable one looks like), and that deleting a
//! promoted draft destroys the only record of the prompts it produced.

use crate::adapters::inbox_store::{
    FrontmatterAuthor, FrontmatterPatch, InboxStore, InboxStoreError,
};
use crate::adapters::projects_registry::ProjectsRegistry;
use crate::domain::model::{DraftStatus, InboxDraft, InboxDraftView, ProjectRef};
use thiserror::Error;

/// Failures callers must distinguish. Everything else surfaces as the
/// underlying [`InboxStoreError`].
#[derive(Debug, Error)]
pub enum InboxServiceError {
    /// Deleting a promoted draft needs an explicit confirmation, because its
    /// `conversions[]` is the only record of the prompts that were sent.
    #[error("draft {0} is promoted; deleting it discards its conversion history")]
    ConfirmationRequired(String),
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

/// Reads and writes drafts under the rules above.
pub struct InboxService {
    store: InboxStore,
    projects: ProjectsRegistry,
}

impl InboxService {
    /// Build over an explicit store and registry.
    pub fn new(store: InboxStore, projects: ProjectsRegistry) -> Self {
        Self { store, projects }
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

    /// Drafts newest first, `Dropped` hidden unless asked for, filtered by
    /// `search` over title and body.
    pub fn list(&self, query: &ListQuery) -> Result<Vec<DraftSummary>, InboxServiceError> {
        let needle = query.search.as_ref().map(|s| s.to_lowercase());
        let mut out = Vec::new();
        for view in self.store.list()? {
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

    /// Every editor-authored frontmatter change goes through here, so the
    /// author is stated once and the store's ownership rules do the rest.
    fn edit(&self, id: &str, patch: FrontmatterPatch) -> Result<InboxDraft, InboxServiceError> {
        Ok(self
            .store
            .merge_frontmatter(id, FrontmatterAuthor::Editor, patch)?)
    }
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
}
