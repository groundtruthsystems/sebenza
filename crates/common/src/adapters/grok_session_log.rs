//! Locate grok (Grok Build) sessions on disk.
//!
//! grok stores each session at `<grok-home>/sessions/<encoded-cwd>/<session-id>/`, where
//! the group directory is the percent-encoded working directory. When that encoded name
//! would exceed 255 bytes grok instead uses a slug plus a hash and records the real path in
//! a `.cwd` file inside the group, so both forms have to be handled: the encoded form is a
//! cheap direct hit, the `.cwd` scan is the fallback.
//!
//! Two lookup directions are needed, and they are deliberately asymmetric:
//!
//! - **by cwd** (`list_session_ids`) for session discovery, which has to walk the group.
//! - **by session id** (`session_dir`) for reading one transcript. A session id is a UUID,
//!   so globbing `sessions/*/<id>/` is unambiguous and sidesteps the encoding rules
//!   entirely. Prefer it whenever the id is known.

use std::path::{Path, PathBuf};

/// `<grok-home>/sessions`. Honours `GROK_HOME`, which grok itself honours.
fn sessions_root() -> Option<PathBuf> {
    let home = match std::env::var_os("GROK_HOME") {
        Some(h) if !h.is_empty() => PathBuf::from(h),
        _ => PathBuf::from(std::env::var_os("HOME")?),
    };
    let root = if std::env::var_os("GROK_HOME").is_some_and(|h| !h.is_empty()) {
        home
    } else {
        home.join(".grok")
    };
    Some(root.join("sessions"))
}

/// Percent-encode `cwd` the way grok names its session group directories: every byte
/// outside the unreserved set becomes uppercase `%XX`, so `/a/b-c.d` -> `%2Fa%2Fb-c.d`.
fn encode_cwd(cwd: &str) -> String {
    let mut out = String::with_capacity(cwd.len() * 3);
    for b in cwd.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The session group directory for `cwd`, or `None` when grok has never run there.
fn group_dir_for_cwd(cwd: &str) -> Option<PathBuf> {
    let root = sessions_root()?;
    let direct = root.join(encode_cwd(cwd));
    if direct.is_dir() {
        return Some(direct);
    }
    // Long-path fallback: grok wrote a slug+hash directory and recorded the real path.
    for entry in std::fs::read_dir(&root).ok()?.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        if let Ok(recorded) = std::fs::read_to_string(dir.join(".cwd"))
            && recorded.trim() == cwd
        {
            return Some(dir);
        }
    }
    None
}

/// Directory entries under `dir` that look like a session, most-recently-modified first.
fn session_dirs_by_mtime(dir: &Path) -> Vec<(String, std::time::SystemTime)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, std::time::SystemTime)> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            // A session directory is named by its id and holds the update stream. Checking
            // for the file keeps sibling bookkeeping directories out of the list.
            if !e.path().join("updates.jsonl").is_file() {
                return None;
            }
            let mtime = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((name, mtime))
        })
        .collect();
    found.sort_by(|a, b| b.1.cmp(&a.1));
    found
}

/// Session ids for `cwd`, newest first. Empty when grok has no sessions there.
pub fn list_session_ids(cwd: &str) -> Vec<String> {
    let Some(group) = group_dir_for_cwd(cwd) else {
        return Vec::new();
    };
    session_dirs_by_mtime(&group)
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

/// The directory holding `session_id`, found by scanning the session groups rather than
/// reconstructing an encoded cwd. `None` when no group contains it.
pub fn session_dir(session_id: &str) -> Option<PathBuf> {
    // Reject anything that could escape the sessions root; ids come from hook payloads.
    if session_id.is_empty()
        || !session_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    let root = sessions_root()?;
    for entry in std::fs::read_dir(&root).ok()?.flatten() {
        let candidate = entry.path().join(session_id);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// The working directory a session group belongs to: its `.cwd` file when present,
/// otherwise its percent-decoded directory name. Used as an integrity check so a
/// mis-recorded session id is reported as "no conversation" rather than as another
/// worktree's transcript.
pub fn group_cwd(group_dir: &Path) -> Option<String> {
    if let Ok(recorded) = std::fs::read_to_string(group_dir.join(".cwd")) {
        return Some(recorded.trim().to_string());
    }
    let name = group_dir.file_name()?.to_string_lossy().to_string();
    let bytes = name.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwd_is_encoded_the_way_grok_names_its_session_groups() {
        // The observed real directory name for /home/kevinbayes/git/oss/sebenza.
        assert_eq!(
            encode_cwd("/home/kevinbayes/git/oss/sebenza"),
            "%2Fhome%2Fkevinbayes%2Fgit%2Foss%2Fsebenza"
        );
        // Dashes and dots stay literal - verified against a real worktree group name.
        assert_eq!(
            encode_cwd("/a/b-c.d_e"),
            "%2Fa%2Fb-c.d_e",
            "unreserved characters must not be encoded, or the direct hit always misses"
        );
    }

    #[test]
    fn group_cwd_round_trips_an_encoded_directory_name() {
        let encoded = encode_cwd("/repo/worktrees/feature-x");
        let dir = PathBuf::from("/tmp").join(&encoded);
        assert_eq!(
            group_cwd(&dir).as_deref(),
            Some("/repo/worktrees/feature-x")
        );
    }

    #[test]
    fn a_session_id_that_could_escape_the_sessions_root_is_refused() {
        // Ids arrive from hook payloads, so they are untrusted input.
        assert!(session_dir("../../etc").is_none());
        assert!(session_dir("a/b").is_none());
        assert!(session_dir("").is_none());
    }
}
