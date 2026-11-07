//! Multiple typed notes inside one inbox item body.
//!
//! An item's body (schema v3) is a sequence of notes, each opened by a marker
//! line at column 0:
//!
//! ```text
//! <!-- sebenza:note id=k3f9a1b2 type=acceptance_criteria title="Acceptance Criteria" -->
//! ```
//!
//! This module is the only code that parses or renders markers, so the store,
//! conversion and the system-agent prompt cannot disagree about them.
//!
//! - **Default note.** A body with no markers is one note, `n1` / `general` /
//!   "Note", and that note alone renders without a marker. Single-note items
//!   therefore stay plain markdown, and a pre-v3 body is already in v3 form.
//! - **Escaping (v3).** A body line that would read as a marker
//!   (`^\\*<!-- sebenza:note`) gains one leading `\` on write; on read a line
//!   matching `^\\+<!-- sebenza:note` loses exactly one. Pre-v3 bodies are
//!   never unescaped.
//! - **Preamble.** v3 text before the first marker becomes a `general` note
//!   with the deterministic id `p` + 8 hex of its hash.
//! - **Malformed markers** (bad grammar, unknown id shape, duplicate id) stay
//!   body text; nothing is ever dropped.
//! - **Hashes** use SHA-1, like the item's `body_hash`: they are optimistic
//!   concurrency tokens, not integrity or security controls.

use serde::Serialize;
use sha1::{Digest, Sha1};

/// The schema version whose bodies use note markers.
pub const NOTES_SCHEMA_VERSION: i32 = 3;
/// The id of the single note a marker-less body reads as.
pub const DEFAULT_NOTE_ID: &str = "n1";
/// The default note's type.
pub const DEFAULT_NOTE_TYPE: &str = "general";
/// The default note's title.
pub const DEFAULT_NOTE_TITLE: &str = "Note";
/// Longest accepted title, in characters.
pub const MAX_TITLE_CHARS: usize = 120;

const MARKER_PREFIX: &str = "<!-- sebenza:note";
const MARKER_SUFFIX: &str = " -->";

/// One note of an inbox item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxNote {
    pub id: String,
    /// A slug, `[a-z0-9_]{1,40}`. `general` and `acceptance_criteria` are
    /// well known; readers ignore types they do not know.
    #[serde(rename = "type")]
    pub note_type: String,
    pub title: String,
    pub body: String,
}

impl InboxNote {
    /// The default note holding `body`.
    pub fn default_with(body: &str) -> Self {
        Self {
            id: DEFAULT_NOTE_ID.to_string(),
            note_type: DEFAULT_NOTE_TYPE.to_string(),
            title: DEFAULT_NOTE_TITLE.to_string(),
            body: body.to_string(),
        }
    }

    /// This note's concurrency token: SHA-1 over type, title and body.
    pub fn hash(&self) -> String {
        let mut h = Sha1::new();
        h.update(self.note_type.as_bytes());
        h.update(b"\n");
        h.update(self.title.as_bytes());
        h.update(b"\n");
        h.update(self.body.as_bytes());
        hex::encode(h.finalize())
    }

    fn is_default_shaped(&self) -> bool {
        self.id == DEFAULT_NOTE_ID
            && self.note_type == DEFAULT_NOTE_TYPE
            && self.title == DEFAULT_NOTE_TITLE
    }
}

/// The token gating structural changes (add, delete, reorder): SHA-1 over each
/// note's `id:hash`, in order. Any note change alters it.
pub fn list_hash(notes: &[InboxNote]) -> String {
    let mut h = Sha1::new();
    for n in notes {
        h.update(n.id.as_bytes());
        h.update(b":");
        h.update(n.hash().as_bytes());
        h.update(b"\n");
    }
    hex::encode(h.finalize())
}

/// True for an acceptable note type slug: `[a-z0-9_]{1,40}`.
pub fn is_valid_type(s: &str) -> bool {
    (1..=40).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// True for an acceptable note id: `[a-z0-9]{2,16}`.
pub fn is_valid_note_id(s: &str) -> bool {
    (2..=16).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// A fresh note id (8 random hex characters) that collides with none of
/// `existing`.
pub fn new_note_id(existing: &[InboxNote]) -> String {
    loop {
        let id = crate::util::id::random_hex(4);
        if is_valid_note_id(&id) && !existing.iter().any(|n| n.id == id) {
            return id;
        }
    }
}

/// The notes in `body`, read as the item's `schema_version` wrote it.
pub fn parse_notes(body: &str, schema_version: i32) -> Vec<InboxNote> {
    let body = body.replace("\r\n", "\n");
    if schema_version < NOTES_SCHEMA_VERSION {
        return vec![InboxNote::default_with(&body)];
    }
    let mut notes: Vec<InboxNote> = Vec::new();
    let mut preamble: Vec<&str> = Vec::new();
    let mut current: Option<(InboxNote, Vec<&str>)> = None;
    for line in split_keep_last(&body) {
        if let Some(note) = parse_marker(line)
            && !notes.iter().any(|n| n.id == note.id)
            && current.as_ref().is_none_or(|(c, _)| c.id != note.id)
        {
            if let Some((mut done, lines)) = current.take() {
                done.body = segment_body(&lines, true);
                notes.push(done);
            }
            current = Some((note, Vec::new()));
            continue;
        }
        match current.as_mut() {
            Some((_, lines)) => lines.push(line),
            None => preamble.push(line),
        }
    }
    if let Some((mut done, lines)) = current.take() {
        done.body = segment_body(&lines, true);
        notes.push(done);
    }
    if notes.is_empty() {
        // No markers: the whole body is the default note, exactly as written.
        return vec![InboxNote::default_with(&segment_body(&preamble, false))];
    }
    let pre = segment_body(&preamble, true);
    if !pre.trim().is_empty() {
        notes.insert(
            0,
            InboxNote {
                id: preamble_id(&pre),
                note_type: DEFAULT_NOTE_TYPE.to_string(),
                title: DEFAULT_NOTE_TITLE.to_string(),
                body: pre,
            },
        );
    }
    notes
}

/// The v3 body for `notes`. A lone default-shaped note renders as its plain
/// (escaped) body; everything else gets a marker per note.
pub fn render_notes(notes: &[InboxNote]) -> String {
    if let [only] = notes
        && only.is_default_shaped()
    {
        return escape_body(&only.body);
    }
    let mut out = String::new();
    for n in notes {
        out.push_str(&render_marker(n));
        out.push('\n');
        out.push_str(&escape_body(&n.body));
        out.push('\n');
    }
    out
}

/// All notes as one markdown document for agents that read a single file
/// (`inbox-note.md`): each note under a `## <title>` heading, in order. A lone
/// default note is written as-is.
pub fn render_concatenated(notes: &[InboxNote]) -> String {
    if let [only] = notes
        && only.is_default_shaped()
    {
        return only.body.clone();
    }
    let mut out = String::new();
    for (i, n) in notes.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("## {}\n\n", one_line(&n.title)));
        out.push_str(n.body.trim_end_matches('\n'));
        out.push('\n');
    }
    out
}

fn one_line(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

/// Lines of `s`, each without its `\n`; a trailing `\n` does not produce an
/// extra empty line, so `segment_body` can restore it exactly.
fn split_keep_last(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = s.split('\n').collect();
    if s.ends_with('\n') {
        lines.pop();
        lines.push("\u{0}EOL");
    }
    lines
}

/// Rebuild a note body from its lines: unescape marker-like lines and, when
/// `separated`, drop the single separator newline `render_notes` adds after
/// each marked body.
fn segment_body(lines: &[&str], separated: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut ends_with_newline = false;
    for line in lines {
        if *line == "\u{0}EOL" {
            ends_with_newline = true;
            continue;
        }
        parts.push(unescape_line(line));
    }
    let mut out = parts.join("\n");
    // Splitting already consumed each line's terminator. A marked body's last
    // terminator is the separator `render_notes` added, so it is not part of
    // the body; an unmarked body keeps its final newline.
    if ends_with_newline && !separated {
        out.push('\n');
    }
    out
}

fn escape_body(body: &str) -> String {
    let body = body.replace("\r\n", "\n");
    body.split('\n')
        .map(|line| {
            if line.trim_start_matches('\\').starts_with(MARKER_PREFIX) {
                format!("\\{line}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn unescape_line(line: &str) -> String {
    let stripped = line.trim_start_matches('\\');
    if line.starts_with('\\') && stripped.starts_with(MARKER_PREFIX) {
        line[1..].to_string()
    } else {
        line.to_string()
    }
}

fn render_marker(n: &InboxNote) -> String {
    let title = serde_json::to_string(&one_line(&n.title))
        .unwrap_or_else(|_| "\"\"".to_string())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e");
    format!(
        "{MARKER_PREFIX} id={} type={} title={title}{MARKER_SUFFIX}",
        n.id, n.note_type
    )
}

/// A strict marker line, or `None` when the line is anything else.
fn parse_marker(line: &str) -> Option<InboxNote> {
    let inner = line
        .strip_prefix(MARKER_PREFIX)?
        .strip_prefix(' ')?
        .strip_suffix(MARKER_SUFFIX)?;
    let rest = inner.strip_prefix("id=")?;
    let (id, rest) = rest.split_once(' ')?;
    let rest = rest.strip_prefix("type=")?;
    let (note_type, rest) = rest.split_once(' ')?;
    let title_json = rest.strip_prefix("title=")?;
    if !is_valid_note_id(id) || !is_valid_type(note_type) || !title_json.starts_with('"') {
        return None;
    }
    let title: String = serde_json::from_str(title_json).ok()?;
    Some(InboxNote {
        id: id.to_string(),
        note_type: note_type.to_string(),
        title,
        body: String::new(),
    })
}

fn preamble_id(text: &str) -> String {
    let digest = hex::encode(Sha1::digest(text.as_bytes()));
    format!("p{}", &digest[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: &str, t: &str, title: &str, body: &str) -> InboxNote {
        InboxNote {
            id: id.into(),
            note_type: t.into(),
            title: title.into(),
            body: body.into(),
        }
    }

    // TS-28: a legacy body is one default note, byte-identical.
    #[test]
    fn a_pre_v3_body_is_one_default_note_unchanged() {
        let body = "# Plan\n\n<!-- sebenza:note id=zz type=x title=\"y\" -->\n\\keep\n";
        let notes = parse_notes(body, 2);
        assert_eq!(notes, vec![InboxNote::default_with(body)]);
    }

    // A single default note stays plain markdown in v3, so legacy bodies are
    // already in v3 form.
    #[test]
    fn a_lone_default_note_renders_without_a_marker() {
        let only = vec![InboxNote::default_with("plain body\n")];
        assert_eq!(render_notes(&only), "plain body\n");
        assert_eq!(parse_notes("plain body\n", 3), only);
    }

    // TS-45: markers, `\`-prefixed and `\\`-prefixed marker-like lines in
    // bodies and titles round-trip byte-identically.
    #[test]
    fn marker_lookalikes_round_trip_byte_identically() {
        let nasty_bodies = [
            "",
            "x",
            "x\n",
            "x\n\n",
            "\n",
            "<!-- sebenza:note id=ab type=general title=\"Forged\" -->",
            "a\n<!-- sebenza:note id=ab type=general title=\"Forged\" -->\nb",
            "\\<!-- sebenza:note id=ab type=general title=\"x\" -->",
            "\\\\<!-- sebenza:note whatever",
            "crlf\r\nline",
        ];
        let titles = ["Plain", "Quote \" and --> and <tag>", "Line\nbreak"];
        for (i, body) in nasty_bodies.iter().enumerate() {
            for title in titles {
                let notes = vec![
                    note("aa11", "acceptance_criteria", title, body),
                    note("bb22", "general", "Second", "tail"),
                ];
                let rendered = render_notes(&notes);
                let back = parse_notes(&rendered, NOTES_SCHEMA_VERSION);
                let mut want = notes.clone();
                want[0].body = body.replace("\r\n", "\n");
                want[0].title = title.replace('\n', " ");
                assert_eq!(back, want, "case {i} title {title:?}:\n{rendered}");
                assert_eq!(render_notes(&back), rendered, "stable re-render");
            }
        }
    }

    // A lone default note with a marker-like line still round-trips.
    #[test]
    fn a_lone_default_note_escapes_marker_lookalikes() {
        let only = vec![InboxNote::default_with(
            "<!-- sebenza:note id=ab type=general title=\"x\" -->\nrest",
        )];
        let rendered = render_notes(&only);
        assert!(rendered.starts_with('\\'), "{rendered}");
        assert_eq!(parse_notes(&rendered, 3), only);
    }

    // TS-46: a malformed or duplicate-id marker is body text, never dropped.
    #[test]
    fn malformed_and_duplicate_markers_stay_body_text() {
        let body = "<!-- sebenza:note id=aa11 type=general title=\"One\" -->\none\n\
<!-- sebenza:note id=aa11 type=general title=\"Dup\" -->\n\
<!-- sebenza:note id=BAD type=general title=\"x\" -->\n\
<!-- sebenza:note id=bb22 type=Bad-Type title=\"x\" -->\n\
<!-- sebenza:note id=cc33 type=general title=unquoted -->\n";
        let notes = parse_notes(body, 3);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, "aa11");
        for kept in ["one", "title=\"Dup\"", "id=BAD", "Bad-Type", "unquoted"] {
            assert!(notes[0].body.contains(kept), "lost {kept:?}");
        }
    }

    // TS-74: a v3 preamble gets the same deterministic id on every read.
    #[test]
    fn a_preamble_gets_a_stable_id() {
        let body = "intro text\n<!-- sebenza:note id=aa11 type=general title=\"One\" -->\none\n";
        let a = parse_notes(body, 3);
        let b = parse_notes(body, 3);
        assert_eq!(a, b);
        assert_eq!(a.len(), 2);
        assert!(
            a[0].id.starts_with('p') && a[0].id.len() == 9,
            "{}",
            a[0].id
        );
        assert!(is_valid_note_id(&a[0].id));
        assert_eq!(a[0].body, "intro text");
        assert_eq!(list_hash(&a), list_hash(&b));
    }

    #[test]
    fn hashes_change_with_type_title_and_body_and_order() {
        let a = note("aa11", "general", "T", "b");
        let mut b = a.clone();
        b.title = "U".into();
        assert_ne!(a.hash(), b.hash());
        let c = note("cc33", "general", "T", "b");
        assert_ne!(
            list_hash(&[a.clone(), c.clone()]),
            list_hash(&[c, a]),
            "order matters"
        );
    }

    #[test]
    fn type_and_id_grammar() {
        assert!(is_valid_type("acceptance_criteria"));
        assert!(!is_valid_type("Acceptance"));
        assert!(!is_valid_type(""));
        assert!(!is_valid_type(&"a".repeat(41)));
        assert!(is_valid_note_id("n1"));
        assert!(!is_valid_note_id("n"));
        assert!(!is_valid_note_id("N1"));
        let existing = vec![note("aa11", "general", "T", "")];
        let id = new_note_id(&existing);
        assert!(is_valid_note_id(&id) && id != "aa11");
    }

    // TS-32: the concatenated document keeps operator order under headings.
    #[test]
    fn the_concatenated_document_keeps_order_under_headings() {
        let notes = vec![
            note("aa11", "general", "Problem", "Graph view.\n"),
            note(
                "bb22",
                "acceptance_criteria",
                "Acceptance Criteria",
                "- shows worktrees",
            ),
        ];
        assert_eq!(
            render_concatenated(&notes),
            "## Problem\n\nGraph view.\n\n## Acceptance Criteria\n\n- shows worktrees\n"
        );
        assert_eq!(
            render_concatenated(&[InboxNote::default_with("as is")]),
            "as is"
        );
    }
}
