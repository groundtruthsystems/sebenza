use super::*;

/// (name, logical lines) of every function in `text`.
fn lengths(language: Language, text: &str) -> Result<Vec<(String, u32)>, Defer> {
    Ok(scan(language, text)?
        .functions
        .into_iter()
        .map(|f| (f.name, f.logical_lines))
        .collect())
}

const RUST_FIXTURE: &str = r####"
//! A fixture with every construct that could fool a brace scanner.
use std::fmt;

/// Generics, a where clause and a nested closure type in the signature.
pub fn generic<'a, T: Into<Vec<u8>>, const N: usize>(
    x: T,
    f: &'a dyn Fn(u8) -> Option<Box<[u8; N]>>,
) -> Result<Vec<u8>, fmt::Error>
where
    T: Clone,
{
    // A comment with a brace { that must not count.
    let s = "a string with } and // inside";
    let raw = r#"a raw string with "quotes" and }"#;
    let c = '{';
    /* block { comment } */
    let label: &'a str = "x";

    println!("{} {}", s, raw);
    let v = vec![c as u8, b'}'];
    Ok(v)
}

macro_rules! make {
    ($name:ident) => {
        fn $name() -> u8 { 1 }
    };
}

fn uses_macros() {
    make!(one);
    let m = format!("{{literal braces}}");
    assert!(m.len() > 0, "message with ) and ]");
}

trait Shape {
    fn area(&self) -> f64;
}
"####;

#[test]
fn ts25_rust_lengths_match_known_counts_despite_strings_macros_and_generics() {
    let got = lengths(Language::Rust, RUST_FIXTURE).unwrap();
    assert_eq!(
        got,
        vec![("generic".into(), 7), ("uses_macros".into(), 3)],
        "the macro_rules template and the bodiless trait method are not functions"
    );
}

const TS_FIXTURE: &str = r#"
import { thing } from "./thing";

/** Generic with an object-type constraint and an object return type. */
export async function load<T extends { id: string }>(
  items: T[],
): Promise<{ count: number }> {
  // a comment } with a brace
  const re = /[{}]\/+/g;
  const tpl = `count: ${items.filter((i) => i.id !== "}").length} }`;
  const s = '{ not a block';

  /* block } */
  return { count: items.length + tpl.length + s.length + (re.test("x") ? 1 : 0) };
}

class Store<K, V> {
  private map = new Map<K, V>();
  get(key: K): V | undefined {
    return this.map.get(key);
  }
}

const handler = (event: { type: string }): void => {
  if (event.type === "a") {
    thing();
  }
};
"#;

#[test]
fn ts25_ts_lengths_match_known_counts_despite_regexes_templates_and_generics() {
    let got = lengths(Language::Js, TS_FIXTURE).unwrap();
    assert_eq!(
        got,
        vec![("load".into(), 4), ("get".into(), 1), ("handler".into(), 3),]
    );
}

const PY_FIXTURE: &str = r#"
import functools


@functools.cache
def decorated(a, b={"k": "}"}):
    """Docstring with def inside: def fake(): pass."""
    # comment
    text = f"{a} and {b['k']}"

    other = """a multi-line
    string that is code"""
    return (text,
            other)


class Model:
    def method(self) -> dict[str, int]:
        return {"a": 1}
"#;

#[test]
fn ts25_python_lengths_match_known_counts_despite_strings_and_decorators() {
    let got = lengths(Language::Python, PY_FIXTURE).unwrap();
    assert_eq!(got, vec![("decorated".into(), 5), ("method".into(), 1)]);
}

#[test]
fn ts25_unreadable_fixtures_defer_rather_than_guess() {
    for (language, text) in [
        (Language::Rust, "fn a() {\n    let s = \"never closed;\n}\n"),
        (Language::Js, "function a() {\n  const r = (/x\n/);\n}\n"),
        (Language::Js, "function f(): { a: 1 } {\n}\n{\n}\n"),
        (Language::Js, "const el = <p>Don't stop</p>;\n"),
        (Language::Python, "def a():\n\tx = 1\n        y = 2\n"),
        (Language::Rust, "fn a() {\n    b(;\n}\n"),
    ] {
        assert!(scan(language, text).is_err(), "{language:?}: {text}");
    }
}

fn rule(id: &str, max: u32) -> Rule {
    Rule {
        rule_id: id.into(),
        category: LONG_METHODS.into(),
        source_doc: "g.md".into(),
        line: 1,
        max_lines: Some(max),
    }
}

fn source(path: &str, text: &str, added: &[u32]) -> SourceFile {
    SourceFile {
        path: path.into(),
        text: Some(text.into()),
        added: added.to_vec(),
        deleted_after: vec![],
    }
}

/// A function named `name` with `n` body lines, starting at line 1.
fn long_fn(language: Language, name: &str, n: u32) -> String {
    let body: String = (0..n)
        .map(|i| match language {
            Language::Python => format!("    x{i} = {i}\n"),
            _ => format!("    let_{i}();\n"),
        })
        .collect();
    match language {
        Language::Rust => format!("fn {name}() {{\n{body}}}\n"),
        Language::Js => format!("function {name}() {{\n{body}}}\n"),
        Language::Python => format!("def {name}():\n{body}"),
    }
}

#[test]
fn ts23_a_changed_function_over_the_limit_is_measured_in_each_language() {
    for (language, path) in [
        (Language::Rust, "src/a.rs"),
        (Language::Js, "web/a.ts"),
        (Language::Python, "tools/a.py"),
    ] {
        let file = source(path, &long_fn(language, "big", 21), &[1, 2]);
        let m = measure(&[rule("short", 20)], &[file]);
        assert_eq!(m.findings.len(), 1, "{language:?}");
        assert_eq!(m.findings[0].rule_id, "short");
        assert_eq!(m.findings[0].path, path);
        assert!(m.findings[0].description.contains("`big`"));
        assert!(m.deferred.is_empty());
        let at_limit = source(path, &long_fn(language, "ok", 20), &[1]);
        assert!(
            measure(&[rule("short", 20)], &[at_limit])
                .findings
                .is_empty()
        );
    }
}

#[test]
fn ts24_a_long_function_with_one_changed_line_is_flagged_at_that_line() {
    let file = source("src/a.rs", &long_fn(Language::Rust, "big", 30), &[17]);
    let m = measure(&[rule("short", 20)], &[file]);
    assert_eq!(m.findings.len(), 1);
    assert_eq!(m.findings[0].line, 17, "anchored to the changed line");

    let untouched = format!("{}fn other() {{}}\n", long_fn(Language::Rust, "big", 30));
    let m = measure(
        &[rule("short", 20)],
        &[source("src/a.rs", &untouched, &[33])],
    );
    assert!(m.findings.is_empty(), "only changed functions are measured");

    let mut lost = source("src/a.rs", &long_fn(Language::Rust, "big", 30), &[]);
    lost.deleted_after = vec![10];
    let m = measure(&[rule("short", 20)], &[lost]);
    assert_eq!(m.findings[0].line, 1, "a deletion anchors on the signature");
}

#[test]
fn ts26_other_languages_unsure_scans_and_opaque_changes_defer_to_the_second_run() {
    let go = source("main.go", "func main() {}\n", &[1]);
    let bad = source("src/b.rs", "fn b() {\n    let s = \"open;\n}\n", &[2]);
    let macro_src = "macro_rules! m {\n    () => {};\n}\n";
    let opaque = source("src/m.rs", macro_src, &[2]);
    let mut unreadable = source("src/c.py", "", &[1]);
    unreadable.text = None;
    let m = measure(&[rule("short", 20)], &[go, bad, opaque, unreadable]);
    assert!(m.findings.is_empty());
    let paths: Vec<_> = m
        .deferred
        .iter()
        .map(|(r, p)| (r.as_str(), p.as_str()))
        .collect();
    assert_eq!(
        paths,
        vec![
            ("short", "main.go"),
            ("short", "src/b.rs"),
            ("short", "src/m.rs"),
            ("short", "src/c.py")
        ]
    );
}

#[test]
fn without_a_long_methods_rule_nothing_is_measured() {
    let mut r = rule("plain", 1);
    r.category = "restating-comments".into();
    r.max_lines = None;
    let m = measure(&[r], &[source("main.go", "x", &[1])]);
    assert_eq!(m, Measurement::default());
    assert_eq!(language_of("a/b.tsx"), Some(Language::Js));
    assert_eq!(language_of("Makefile"), None);
}
