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
/// The first declaration of a rule id wins.
pub fn build_manifest(standards_rev: &str, docs: &[StandardsDoc]) -> (RuleManifest, Vec<Blocker>) {
    let mut rules: Vec<Rule> = Vec::new();
    let mut blockers = Vec::new();
    for doc in docs {
        for parsed in parse_rules(&doc.path, &doc.text) {
            match parsed {
                Err(line) => {
                    blockers.push(Blocker::at_line(Reason::MalformedRule, &doc.path, line))
                }
                Ok(rule) if rules.iter().any(|r| r.rule_id == rule.rule_id) => blockers.push(
                    Blocker::at_line(Reason::DuplicateRule, &rule.source_doc, rule.line),
                ),
                Ok(rule) => rules.push(rule),
            }
        }
    }
    blockers.sort();
    let manifest = RuleManifest {
        standards_rev: standards_rev.to_string(),
        rules,
    };
    (manifest, blockers)
}

/// The rules `text` declares; `Err` holds the line of a malformed tag.
pub fn parse_rules(source_doc: &str, text: &str) -> Vec<Result<Rule, usize>> {
    let mut fence: Option<&str> = None;
    let mut parsed = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(marker) = fence_marker(trimmed) {
            fence = match fence {
                Some(open) if open == marker => None,
                None => Some(marker),
                still_open => still_open,
            };
            continue;
        }
        let tag = strip_list_marker(trimmed).strip_prefix("[rule:");
        if let (None, Some(tag)) = (fence, tag) {
            parsed.push(
                parse_tag(tag)
                    .map(|t| t.into_rule(source_doc, i + 1))
                    .ok_or(i + 1),
            );
        }
    }
    parsed
}

fn fence_marker(line: &str) -> Option<&'static str> {
    ["```", "~~~"].into_iter().find(|m| line.starts_with(m))
}

fn strip_list_marker(line: &str) -> &str {
    if let Some(rest) = ["- ", "* ", "+ "].iter().find_map(|m| line.strip_prefix(m)) {
        return rest.trim_start();
    }
    let rest = line.trim_start_matches(|c: char| c.is_ascii_digit());
    match rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
        Some(item) if rest.len() < line.len() => item.trim_start(),
        _ => line,
    }
}

/// The attributes of one well-formed tag.
struct Tag<'a> {
    id: &'a str,
    category: &'a str,
    max_lines: Option<u32>,
}

impl Tag<'_> {
    fn into_rule(self, source_doc: &str, line: usize) -> Rule {
        Rule {
            rule_id: self.id.to_string(),
            category: self.category.to_string(),
            source_doc: source_doc.to_string(),
            line,
            max_lines: self.max_lines,
        }
    }
}

/// Parse the text after `[rule:` up to the closing `]`.
fn parse_tag(after_prefix: &str) -> Option<Tag<'_>> {
    let (body, _prose) = after_prefix.split_once(']')?;
    let mut tokens = body.split(' ').filter(|t| !t.is_empty());
    let id = tokens
        .next()
        .filter(|id| body.starts_with(id) && valid_name(id) && !RESERVED_RULE_IDS.contains(id))?;
    let (mut category, mut max_lines) = (None, None);
    for token in tokens {
        match token.split_once('=')? {
            ("category", v) if valid_name(v) && category.is_none() => category = Some(v),
            ("max_lines", v) if max_lines.is_none() => {
                max_lines = Some(v.parse::<u32>().ok().filter(|n| *n > 0)?)
            }
            _ => return None,
        }
    }
    let category = category?;
    ((category == LONG_METHODS) == max_lines.is_some()).then_some(Tag {
        id,
        category,
        max_lines,
    })
}

fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
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

    /// The three default rules as `(category, rule id, max_lines)`.
    fn defaults(path: &str, text: &str) -> Vec<(String, String, Option<u32>)> {
        let (manifest, blockers) = build_manifest("r", &[doc(path, text)]);
        assert!(blockers.is_empty(), "{path}: {blockers:?}");
        let mut rules: Vec<_> = manifest
            .rules
            .into_iter()
            .map(|r| (r.category, r.rule_id, r.max_lines))
            .collect();
        rules.sort();
        rules
    }

    #[test]
    fn ts46_the_setup_template_ships_the_three_default_rules_and_this_repository_adopts_them() {
        let template = defaults(
            "template",
            include_str!(
                "../../../../../skills/skills/sebenza-setup/assets/code_styleguides/general.md"
            ),
        );
        let categories: Vec<&str> = template.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(
            categories,
            vec![EXCESSIVE_COMMENTS, LONG_METHODS, RESTATING_COMMENTS]
        );
        assert_eq!(template[1].2, Some(20), "D-03: 20 logical lines");
        let here = defaults(
            "here",
            include_str!("../../../../../.ai/sebenza/code_styleguides/general.md"),
        );
        assert_eq!(here, template);
    }
}
