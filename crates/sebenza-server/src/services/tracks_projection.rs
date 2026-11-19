//! The `tracks.json` projection of a completion (FR-02): `status: "done"`.
//!
//! Skills also write `tracks.json`, without the daemon's lock, so the edit is
//! surgical and guarded. Only the track's `status` and `updated_at` value
//! bytes change; every other byte, including formatting, stays as the skill
//! wrote it. The write holds an advisory lock on `.ai/sebenza/`, re-reads the
//! file before replacing it and retries when its content hash moved (CAS).
#![allow(unused_imports, unused_variables)] // red: stubs until phase-1-task-2

use common::domain::solution::sha256_hex;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;

/// How many times a projection re-reads after losing a race.
pub const CAS_ATTEMPTS: usize = 5;

/// What a projection did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Projected {
    Applied,
    AlreadyDone,
}

/// Why a projection could not be applied.
#[derive(Debug, thiserror::Error)]
pub enum ProjectionError {
    #[error("track {0} is not in tracks.json")]
    TrackNotFound(String),
    #[error("tracks.json is malformed: {0}")]
    Malformed(String),
    #[error("tracks.json kept changing; gave up after {CAS_ATTEMPTS} attempts")]
    Contended,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Every `(track_id, status)` in a `tracks.json`.
pub fn track_statuses(path: &Path) -> Result<Vec<(String, String)>, ProjectionError> {
    todo!("phase-1-task-2")
}

/// Project `status: "done"` for `track_id` into the `tracks.json` at `path`.
pub fn project_done(path: &Path, track_id: &str) -> Result<Projected, ProjectionError> {
    todo!("phase-1-task-2")
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Temp file in the same directory, fsynced, renamed over `path`.
fn replace_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".tracks.json.{}.tmp",
        common::util::id::random_ulid()
    ));
    let mut f = fs::File::create(&tmp)?;
    if let Ok(meta) = fs::metadata(path) {
        let _ = f.set_permissions(meta.permissions());
    }
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, path)?;
    fs::File::open(dir)?.sync_all()
}

/// `text` with `track_id` marked done at `now`, or `None` when it already is.
/// The result is checked to differ from `text` in exactly those two fields.
pub fn mark_done_text(
    text: &str,
    track_id: &str,
    now: &str,
) -> Result<Option<String>, ProjectionError> {
    todo!("phase-1-task-2")
}

/// A member of a JSON object: its key and the byte span of its value.
#[derive(Debug)]
struct Member {
    key: String,
    start: usize,
    end: usize,
}

fn edits_for(track: &[Member], now: &str) -> Result<Vec<(usize, usize, String)>, ProjectionError> {
    let status = track
        .iter()
        .find(|m| m.key == "status")
        .ok_or_else(|| ProjectionError::Malformed("track has no status".into()))?;
    let stamp = quote(now);
    let mut edits = vec![(status.start, status.end, "\"done\"".to_string())];
    match track.iter().find(|m| m.key == "updated_at") {
        Some(m) => edits.push((m.start, m.end, stamp)),
        None => edits.push((status.end, status.end, format!(", \"updated_at\": {stamp}"))),
    }
    Ok(edits)
}

fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
}

fn apply_edits(text: &str, mut edits: Vec<(usize, usize, String)>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = text.to_string();
    for (start, end, with) in edits {
        out.replace_range(start..end, &with);
    }
    out
}

/// The edited document must equal the original with only the two fields set.
fn verify(before: &str, after: &str, track_id: &str, now: &str) -> Result<(), ProjectionError> {
    let parse = |t: &str| {
        serde_json::from_str::<Value>(t).map_err(|e| ProjectionError::Malformed(e.to_string()))
    };
    let mut expected = parse(before)?;
    if let Some(t) = expected["tracks"]
        .as_array_mut()
        .and_then(|ts| ts.iter_mut().find(|t| t["track_id"] == track_id))
    {
        t["status"] = Value::from("done");
        t["updated_at"] = Value::from(now);
    }
    if parse(after)? == expected {
        Ok(())
    } else {
        Err(ProjectionError::Malformed("surgical edit diverged".into()))
    }
}

/// The members of the first object in `tracks` whose `track_id` matches.
fn find_track(b: &[u8], track_id: &str) -> Result<Vec<Member>, ProjectionError> {
    let root = skip_ws(b, 0);
    let tracks = members(b, root)?
        .into_iter()
        .find(|m| m.key == "tracks")
        .ok_or_else(|| ProjectionError::Malformed("no tracks array".into()))?;
    for (start, _) in elements(b, tracks.start)? {
        let obj = members(b, start)?;
        let id = obj.iter().find(|m| m.key == "track_id");
        if id.is_some_and(|m| decode(b, m) == Some(track_id.to_string())) {
            return Ok(obj);
        }
    }
    Err(ProjectionError::TrackNotFound(track_id.to_string()))
}

fn decode(b: &[u8], m: &Member) -> Option<String> {
    serde_json::from_slice::<String>(&b[m.start..m.end]).ok()
}

fn malformed(at: usize) -> ProjectionError {
    ProjectionError::Malformed(format!("unexpected input at byte {at}"))
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// The byte after the string starting at `i` (which must be `"`).
fn string_end(b: &[u8], i: usize) -> Result<usize, ProjectionError> {
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return Ok(j + 1),
            _ => j += 1,
        }
    }
    Err(malformed(i))
}

/// The byte after the value starting at `i`.
fn value_end(b: &[u8], i: usize) -> Result<usize, ProjectionError> {
    match b.get(i) {
        Some(b'"') => string_end(b, i),
        Some(b'{') | Some(b'[') => container_end(b, i),
        Some(_) => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']') && !b[j].is_ascii_whitespace()
            {
                j += 1;
            }
            Ok(j)
        }
        None => Err(malformed(i)),
    }
}

fn container_end(b: &[u8], i: usize) -> Result<usize, ProjectionError> {
    let mut depth = 0usize;
    let mut j = i;
    while j < b.len() {
        match b[j] {
            b'"' => {
                j = string_end(b, j)?;
                continue;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(j + 1);
                }
            }
            _ => {}
        }
        j += 1;
    }
    Err(malformed(i))
}

/// The members of the object starting at `i`.
fn members(b: &[u8], i: usize) -> Result<Vec<Member>, ProjectionError> {
    if b.get(i) != Some(&b'{') {
        return Err(malformed(i));
    }
    let mut out = Vec::new();
    let mut j = skip_ws(b, i + 1);
    while b.get(j) == Some(&b'"') {
        let key_end = string_end(b, j)?;
        let key = serde_json::from_slice::<String>(&b[j..key_end]).map_err(|_| malformed(j))?;
        let colon = skip_ws(b, key_end);
        if b.get(colon) != Some(&b':') {
            return Err(malformed(colon));
        }
        let start = skip_ws(b, colon + 1);
        let end = value_end(b, start)?;
        out.push(Member { key, start, end });
        j = skip_ws(b, end);
        if b.get(j) == Some(&b',') {
            j = skip_ws(b, j + 1);
        }
    }
    Ok(out)
}

/// The spans of the elements of the array starting at `i`.
fn elements(b: &[u8], i: usize) -> Result<Vec<(usize, usize)>, ProjectionError> {
    if b.get(i) != Some(&b'[') {
        return Err(malformed(i));
    }
    let mut out = Vec::new();
    let mut j = skip_ws(b, i + 1);
    while j < b.len() && b[j] != b']' {
        let end = value_end(b, j)?;
        out.push((j, end));
        j = skip_ws(b, end);
        if b.get(j) == Some(&b',') {
            j = skip_ws(b, j + 1);
        }
    }
    Ok(out)
}

/// Test-only fault injection between the read and the CAS check.
#[cfg(test)]
pub mod hooks {
    use std::cell::RefCell;
    use std::path::Path;

    type Hook = Box<dyn FnMut(&Path)>;
    thread_local! {
        static BEFORE_CAS: RefCell<Option<Hook>> = RefCell::new(None);
    }

    /// Run `hook` on this thread just before each CAS check.
    pub fn set_before_cas(hook: impl FnMut(&Path) + 'static) {
        BEFORE_CAS.with(|h| *h.borrow_mut() = Some(Box::new(hook)));
    }

    pub fn clear() {
        BEFORE_CAS.with(|h| *h.borrow_mut() = None);
    }

    pub(super) fn before_cas(path: &Path) {
        BEFORE_CAS.with(|h| {
            if let Some(hook) = h.borrow_mut().as_mut() {
                hook(path);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::{OTHER_TRACK, TRACK, tracks_json};

    const NOW: &str = "2026-11-19T12:00:00Z";

    fn temp_tracks(name: &str, text: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sebenza-projection-{}-{name}-{}",
            std::process::id(),
            common::util::id::random_ulid()
        ));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("tracks.json");
        fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn only_the_status_and_updated_at_bytes_change() {
        let before = tracks_json("doing", "feat");
        let after = mark_done_text(&before, TRACK, NOW).unwrap().unwrap();
        let expected = before.replacen(
            "\"status\": \"doing\",\n      \"updated_at\": \"2026-11-18T00:00:00Z\"",
            "\"status\": \"done\",\n      \"updated_at\": \"2026-11-19T12:00:00Z\"",
            1,
        );
        assert_ne!(expected, before, "fixture shape changed");
        assert_eq!(after, expected);
    }

    #[test]
    fn an_already_done_track_needs_no_write() {
        let text = tracks_json("done", "feat");
        assert!(mark_done_text(&text, TRACK, NOW).unwrap().is_none());
    }

    #[test]
    fn a_missing_updated_at_is_added_beside_status() {
        let text = r#"{"tracks":[{"track_id":"t_20261118","status":"doing","x":[1,{"y":"}"}]}]}"#;
        let after = mark_done_text(text, TRACK, NOW).unwrap().unwrap();
        let v: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(v["tracks"][0]["status"], "done");
        assert_eq!(v["tracks"][0]["updated_at"], NOW);
        assert_eq!(v["tracks"][0]["x"][1]["y"], "}");
    }

    #[test]
    fn an_unknown_track_is_refused() {
        let err = mark_done_text(&tracks_json("doing", "feat"), "nope", NOW).unwrap_err();
        assert!(matches!(err, ProjectionError::TrackNotFound(_)));
    }

    #[test]
    fn projecting_writes_the_file_and_leaves_other_tracks_alone() {
        let p = temp_tracks("write", &tracks_json("doing", "feat"));
        assert_eq!(project_done(&p, TRACK).unwrap(), Projected::Applied);
        let statuses = track_statuses(&p).unwrap();
        assert_eq!(
            statuses,
            vec![
                (OTHER_TRACK.to_string(), "doing".to_string()),
                (TRACK.to_string(), "done".to_string())
            ]
        );
        assert_eq!(project_done(&p, TRACK).unwrap(), Projected::AlreadyDone);
    }

    #[test]
    fn ts67_a_concurrent_skill_edit_is_retried_not_lost() {
        let p = temp_tracks("cas", &tracks_json("doing", "feat"));
        let mut fired = false;
        hooks::set_before_cas(move |path| {
            if !fired {
                fired = true;
                let text = fs::read_to_string(path).unwrap();
                fs::write(
                    path,
                    text.replace("\"Other\"", "\"Other, edited by a skill\""),
                )
                .unwrap();
            }
        });
        let result = project_done(&p, TRACK);
        hooks::clear();
        assert_eq!(result.unwrap(), Projected::Applied);
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("Other, edited by a skill"), "skill edit kept");
        assert_eq!(track_statuses(&p).unwrap()[1].1, "done");
    }

    #[test]
    fn a_projection_that_never_wins_the_race_gives_up() {
        let p = temp_tracks("contended", &tracks_json("doing", "feat"));
        let mut n = 0;
        hooks::set_before_cas(move |path| {
            n += 1;
            fs::write(path, tracks_json("doing", &format!("b{n}"))).unwrap();
        });
        let result = project_done(&p, TRACK);
        hooks::clear();
        assert!(matches!(result, Err(ProjectionError::Contended)));
    }
}
