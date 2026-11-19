//! The daemon's numeric-rule measurers (FR-15, D-03): in-tree method-length
//! scanners for Rust, TS/JS and Python, with no parser crates.
//!
//! A function's length is its logical lines: body lines holding code, not
//! counting blank lines, comment-only lines or the signature line. For a
//! brace language the body is what lies between the braces, less the line
//! of the opening brace; for Python it is the logical lines indented under
//! the `def`, less a leading docstring.
//!
//! A function is changed when an in-scope hunk adds a line inside it, or
//! deletes lines from inside it. A changed function over a `long-methods`
//! rule's `max_lines` is a [`MeasuredFinding`] anchored on its first added
//! line (or its signature line when it only lost lines). The finding stands
//! whatever the run dispositioned. A file in another language, a file the
//! scanner is unsure of, or a change inside an opaque range (a
//! `macro_rules!` body) is deferred: its (rule, file) pair goes to the
//! second run.
//!
//! File contents come from git objects at the job's head only.

mod js;
mod lexer;
mod python;
mod rust;
pub mod tokens;

use super::deriver::MeasuredFinding;
use super::git_pinned::{Git, GitError};
use super::manifest::{LONG_METHODS, Rule};
use super::objects::{blob_sizes, read_blobs};
use super::scope::DiffFile;
use tokens::{Defer, Function, Scanned, display_name};

/// A language with a measurer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    /// TypeScript and JavaScript, with or without JSX.
    Js,
    Python,
}

/// The measurer language of `path`, by extension.
pub fn language_of(path: &str) -> Option<Language> {
    let ext = path.rsplit_once('.')?.1;
    match ext {
        "rs" => Some(Language::Rust),
        "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => Some(Language::Js),
        "py" | "pyi" => Some(Language::Python),
        _ => None,
    }
}

/// Every function in `text`, or why the scanner defers.
pub fn scan(language: Language, text: &str) -> Result<Scanned, Defer> {
    match language {
        Language::Rust => rust::scan(text),
        Language::Js => js::scan(text),
        Language::Python => python::scan(text),
    }
}

/// One in-scope file at head, as the measurers read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub path: String,
    /// The file's text at head; `None` when unreadable or not UTF-8.
    pub text: Option<String>,
    /// Head lines the scope diff added.
    pub added: Vec<u32>,
    /// Head lines after which the scope diff deleted lines.
    pub deleted_after: Vec<u32>,
}

impl SourceFile {
    /// `file`'s changed lines, with `text` as read at head.
    pub fn from_diff(file: &DiffFile, text: Option<String>) -> SourceFile {
        let (mut added, mut deleted_after) = (Vec::new(), Vec::new());
        for h in &file.hunks {
            if h.new_lines == 0 {
                deleted_after.push(h.new_start);
            } else {
                added.extend(h.new_start..h.new_start + h.new_lines);
            }
        }
        SourceFile {
            path: file.path().to_string(),
            text,
            added,
            deleted_after,
        }
    }

    /// The first changed line inside `f`, if `f` was changed.
    fn changed_line_in(&self, f: &Function) -> Option<u32> {
        let inside = |l: &&u32| (f.start_line..=f.end_line).contains(*l);
        let lost = |l: &&u32| (f.start_line..f.end_line).contains(*l);
        match self.added.iter().find(inside) {
            Some(l) => Some(*l),
            None => self.deleted_after.iter().find(lost).map(|_| f.start_line),
        }
    }

    /// Whether a changed line falls in an opaque range outside any function.
    fn changes_opaque(&self, s: &Scanned) -> bool {
        let lines = self.added.iter().chain(&self.deleted_after);
        lines.into_iter().any(|l| {
            let opaque = s.opaque.iter().any(|(a, b)| (a..=b).contains(&l));
            opaque
                && !s
                    .functions
                    .iter()
                    .any(|f| (f.start_line..=f.end_line).contains(l))
        })
    }
}

/// What the measurers decided for one job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Measurement {
    pub findings: Vec<MeasuredFinding>,
    /// (rule id, path) pairs the second run must judge.
    pub deferred: Vec<(String, String)>,
}

/// Measure every `long-methods` rule of `rules` over `files`.
pub fn measure(rules: &[Rule], files: &[SourceFile]) -> Measurement {
    let long: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.category == LONG_METHODS)
        .collect();
    let mut out = Measurement::default();
    if long.is_empty() {
        return out;
    }
    for file in files {
        match functions_of(file) {
            Some(functions) => {
                let found = long.iter().flat_map(|r| breaches(r, file, &functions));
                out.findings.extend(found);
            }
            None => {
                let pairs = long.iter().map(|r| (r.rule_id.clone(), file.path.clone()));
                out.deferred.extend(pairs);
            }
        }
    }
    out
}

/// The functions of `file`, or `None` when the file defers.
fn functions_of(file: &SourceFile) -> Option<Vec<Function>> {
    let language = language_of(&file.path)?;
    let scanned = scan(language, file.text.as_deref()?).ok()?;
    (!file.changes_opaque(&scanned)).then_some(scanned.functions)
}

fn breaches(rule: &Rule, file: &SourceFile, functions: &[Function]) -> Vec<MeasuredFinding> {
    let Some(max) = rule.max_lines else {
        return Vec::new();
    };
    functions
        .iter()
        .filter(|f| f.logical_lines > max)
        .filter_map(|f| {
            Some(MeasuredFinding {
                rule_id: rule.rule_id.clone(),
                path: file.path.clone(),
                line: file.changed_line_in(f)?,
                description: format!(
                    "Function `{}` has more than {max} logical lines in its body (rule `{}`).",
                    display_name(&f.name),
                    rule.rule_id
                ),
            })
        })
        .collect()
}

/// Read each of `files` at head from git objects, as [`SourceFile`]s. Files
/// with no measurer, or over `max_blob_bytes`, are not read and defer.
pub fn read_sources(
    git: &Git,
    files: &[&DiffFile],
    max_blob_bytes: u64,
) -> Result<Vec<SourceFile>, GitError> {
    let oids: Vec<String> = files
        .iter()
        .filter(|f| language_of(f.path()).is_some())
        .filter_map(|f| f.new_oid.clone())
        .collect();
    let sizes = blob_sizes(git, &oids)?;
    let readable: Vec<String> = oids
        .into_iter()
        .filter(|o| sizes.get(o).is_some_and(|s| *s <= max_blob_bytes))
        .collect();
    let total = readable.iter().filter_map(|o| sizes.get(o)).sum::<u64>() as usize;
    let blobs = read_blobs(git, &readable, total)?;
    Ok(files
        .iter()
        .map(|f| {
            let text = f.new_oid.as_ref().and_then(|o| blobs.get(o));
            let text = text.and_then(|b| String::from_utf8(b.clone()).ok());
            SourceFile::from_diff(f, text)
        })
        .collect())
}

#[cfg(test)]
mod tests;
