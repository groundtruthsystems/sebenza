//! The review agent's working directory (NFR-04, D-04): a read-only export
//! of the job's head snapshot, built from git objects only (`ls-tree` and
//! `cat-file`), never from the live worktree.
//!
//! Only regular blobs are written. Symlinks, submodules, `.git`, secrets
//! files (`.env*`, `*.pem`, `id_*` keys) and agent-instruction files
//! (`CLAUDE.md`, `AGENTS.md`, `.claude/`) are left out, as are blobs over the
//! blob cap. Files end up 0444 in 0555 directories.

use crate::services::review_snapshot::{excluded, make_read_only};
use crate::services::standards_review::git_pinned::{Git, GitError};
use crate::services::standards_review::objects::{blob_sizes, read_blobs};
use std::path::Path;

/// Blobs read per `cat-file --batch` call.
const BATCH: usize = 256;

/// One regular file of the head tree.
struct Entry {
    oid: String,
    path: String,
}

/// Why an export could not be built.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("export: {0}")]
    Io(#[from] std::io::Error),
}

/// Write the tree of `head` into `dest` (created fresh) and make it
/// read-only. Returns the exported paths.
pub fn export_head(
    git: &Git,
    head: &str,
    dest: &Path,
    max_blob_bytes: u64,
) -> Result<Vec<String>, ExportError> {
    let listing = git.run(&["ls-tree", "-r", "-z", "--full-tree", head])?;
    let entries: Vec<Entry> = listing
        .split(|b| *b == 0)
        .filter_map(parse_entry)
        .filter(|e| exportable(&e.path))
        .collect();
    let oids: Vec<String> = entries.iter().map(|e| e.oid.clone()).collect();
    let sizes = blob_sizes(git, &oids)?;
    let wanted: Vec<&Entry> = entries
        .iter()
        .filter(|e| sizes.get(&e.oid).is_some_and(|s| *s <= max_blob_bytes))
        .collect();
    std::fs::create_dir_all(dest)?;
    let mut written = Vec::new();
    for chunk in wanted.chunks(BATCH) {
        written.extend(write_chunk(git, chunk, &sizes, dest)?);
    }
    make_read_only(dest);
    Ok(written)
}

fn write_chunk(
    git: &Git,
    chunk: &[&Entry],
    sizes: &std::collections::BTreeMap<String, u64>,
    dest: &Path,
) -> Result<Vec<String>, ExportError> {
    let oids: Vec<String> = chunk.iter().map(|e| e.oid.clone()).collect();
    let total = oids.iter().filter_map(|o| sizes.get(o)).sum::<u64>() as usize;
    let blobs = read_blobs(git, &oids, total)?;
    let mut written = Vec::new();
    for e in chunk {
        let Some(bytes) = blobs.get(&e.oid) else {
            continue;
        };
        let to = dest.join(&e.path);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&to, bytes)?;
        written.push(e.path.clone());
    }
    Ok(written)
}

/// `<mode> blob <oid>\t<path>` for a regular file; anything else is `None`.
fn parse_entry(raw: &[u8]) -> Option<Entry> {
    let line = std::str::from_utf8(raw).ok()?;
    let (meta, path) = line.split_once('\t')?;
    let mut f = meta.split(' ');
    let (mode, kind, oid) = (f.next()?, f.next()?, f.next()?);
    let regular = kind == "blob" && matches!(mode, "100644" | "100755");
    regular.then(|| Entry {
        oid: oid.to_string(),
        path: path.to_string(),
    })
}

/// A relative, normal path that is neither excluded nor named `id_*`.
fn exportable(path: &str) -> bool {
    let normal = !path.starts_with('/')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    let name = path.rsplit('/').next().unwrap_or(path);
    normal && !excluded(path) && !name.starts_with("id_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_regular_safe_blobs_are_exported() {
        let entry = |s: &str| parse_entry(s.as_bytes()).map(|e| e.path);
        assert_eq!(entry("100644 blob abc\tsrc/a.rs"), Some("src/a.rs".into()));
        assert_eq!(entry("100755 blob abc\tbin/run"), Some("bin/run".into()));
        assert_eq!(entry("120000 blob abc\tlink"), None, "a symlink");
        assert_eq!(entry("160000 commit abc\tsub"), None, "a submodule");
        for bad in [
            "../x",
            "/etc/passwd",
            "a//b",
            ".git/config",
            ".env.local",
            "k/id_rsa",
            "keys/id_dsa",
            "cert.pem",
            "CLAUDE.md",
            "docs/AGENTS.md",
            ".claude/settings.json",
        ] {
            assert!(!exportable(bad), "{bad}");
        }
        assert!(exportable("src/lib.rs"));
    }
}
