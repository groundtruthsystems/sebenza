//! The rule manifest: the only standards a review enforces (D-01, D-02). The
//! daemon and skills hard-code no rule; a rule exists only where a listed
//! standards document declares it with a rule tag.
//!
//! # Rule syntax
//!
//! A rule tag opens a line, optionally after a markdown list marker (`-`,
//! `*`, `+`, `1.` or `1)`), and the rest of the line is the rule's prose:
//!
//! ```text
//! - [rule:long-methods category=long-methods max_lines=20] Keep function bodies to 20 logical lines.
//! - [rule:no-restating-comments category=restating-comments] Comments explain why, never restate the code.
//! ```
//!
//! - `rule:<id>` comes first. An id is 1–64 characters of `a-z`, `0-9` and
//!   `-`, starting with a letter or digit, and is unique across the
//!   manifest. `standards-change` and `out-of-scope-edit` are reserved.
//! - `category=<category>` is required, with the same character set. The
//!   categories the review knows are `restating-comments`,
//!   `excessive-comments` and `long-methods`; any other category is still a
//!   rule the review dispositions.
//! - `max_lines=<n>` is a positive integer, required for `long-methods` and
//!   refused elsewhere. It counts logical lines of a function body.
//! - Attributes are separated by spaces; no other attribute is accepted.
//!
//! Tags inside fenced code blocks are examples and are ignored, as are tags
//! that do not open their line. A line that opens with `[rule:` but breaks
//! these rules is a `malformed-rule` blocker, so a typo cannot silently
//! drop a standard.

use super::reasons::{Blocker, Reason};
use super::standards_docs::StandardsDoc;
use serde::Serialize;

/// The comment category whose rule forbids comments restating the code.
pub const RESTATING_COMMENTS: &str = "restating-comments";
/// The comment category whose rule limits comment density.
pub const EXCESSIVE_COMMENTS: &str = "excessive-comments";
/// The numeric category whose rule caps function body length.
pub const LONG_METHODS: &str = "long-methods";
/// Rule ids the daemon raises itself; no document may declare them.
pub const RESERVED_RULE_IDS: [&str; 2] = ["standards-change", "out-of-scope-edit"];

/// One declared rule and where it is declared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rule {
    pub rule_id: String,
    pub category: String,
    pub source_doc: String,
    /// 1-based line of the tag in `source_doc`.
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<u32>,
}

/// Every rule at one standards revision.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RuleManifest {
    pub standards_rev: String,
    pub rules: Vec<Rule>,
}

/// The manifest of `docs`, plus a blocker per malformed or duplicate tag.
pub fn build_manifest(standards_rev: &str, docs: &[StandardsDoc]) -> (RuleManifest, Vec<Blocker>) {
    todo!("phase-2-task-2")
}

/// The rules `text` declares; `Err` lines are malformed tags.
pub fn parse_rules(source_doc: &str, text: &str) -> Vec<Result<Rule, usize>> {
    todo!("phase-2-task-2")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, text: &str) -> StandardsDoc {
        StandardsDoc {
            path: path.into(),
            blob: "b".into(),
            text: text.into(),
        }
    }

    fn ok_rules(text: &str) -> Vec<Rule> {
        parse_rules("g.md", text).into_iter().flatten().collect()
    }

    #[test]
    fn a_tagged_list_item_declares_a_rule() {
        let text = "# Style\n\n- [rule:long-methods category=long-methods max_lines=20] Keep it short.\n\
                    * [rule:why-comments category=restating-comments] Explain why.\n\
                    3. [rule:few-comments category=excessive-comments] Sparingly.\n\
                    [rule:naming category=naming] Bare tag.\n";
        let rules = ok_rules(text);
        assert_eq!(
            rules[0],
            Rule {
                rule_id: "long-methods".into(),
                category: LONG_METHODS.into(),
                source_doc: "g.md".into(),
                line: 3,
                max_lines: Some(20),
            }
        );
        let ids: Vec<_> = rules.iter().map(|r| (r.rule_id.as_str(), r.line)).collect();
        assert_eq!(
            ids,
            vec![
                ("long-methods", 3),
                ("why-comments", 4),
                ("few-comments", 5),
                ("naming", 6)
            ]
        );
    }

    #[test]
    fn prose_mentions_and_fenced_examples_are_not_rules() {
        let text = "Write `[rule:x category=y]` tags.\nSee [rule:x category=y] above.\n\
                    ```text\n- [rule:example category=long-methods max_lines=5] e\n```\n\
                    ~~~\n[rule:also category=z]\n~~~\n";
        assert!(parse_rules("g.md", text).is_empty());
    }

    #[test]
    fn malformed_tags_are_reported_by_line() {
        for bad in [
            "- [rule: category=x] no id",
            "- [rule:Bad_Id category=x] case",
            "- [rule:x] no category",
            "- [rule:x category=long-methods] no threshold",
            "- [rule:x category=long-methods max_lines=0] zero",
            "- [rule:x category=long-methods max_lines=ten] word",
            "- [rule:x category=naming max_lines=5] threshold elsewhere",
            "- [rule:x category=y colour=red] unknown attribute",
            "- [rule:x category=y unclosed",
            "- [rule:standards-change category=y] reserved",
        ] {
            assert_eq!(parse_rules("g.md", bad), vec![Err(1)], "{bad}");
        }
    }

    #[test]
    fn the_manifest_spans_docs_and_flags_duplicates() {
        let docs = [
            doc("a.md", "- [rule:one category=x] a\n"),
            doc(
                "b.md",
                "\n- [rule:one category=y] b\n- [rule:two category=x] c\n- [rule:bad] d\n",
            ),
        ];
        let (m, blockers) = build_manifest("rev1", &docs);
        assert_eq!(m.standards_rev, "rev1");
        let ids: Vec<_> = m
            .rules
            .iter()
            .map(|r| (r.rule_id.as_str(), r.source_doc.as_str()))
            .collect();
        assert_eq!(ids, vec![("one", "a.md"), ("two", "b.md")]);
        assert_eq!(
            blockers,
            vec![
                Blocker::at_line(Reason::MalformedRule, "b.md", 4),
                Blocker::at_line(Reason::DuplicateRule, "b.md", 2),
            ]
        );
    }

    #[test]
    fn a_manifest_holds_no_standards_text() {
        let docs = [doc("a.md", "- [rule:one category=x] SECRET-PROSE\n")];
        let json = serde_json::to_string(&build_manifest("r", &docs).0).unwrap();
        assert!(!json.contains("SECRET-PROSE"));
    }
}
