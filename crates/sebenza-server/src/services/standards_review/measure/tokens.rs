//! The token stream the language scanners share, and the helpers that pair
//! brackets and count the logical lines a run of tokens covers.

use std::collections::BTreeSet;

/// Why a scanner will not give a length: the rule goes to the second run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Defer(pub &'static str);

/// What a token is, as far as function detection cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ident,
    Punct,
    /// A string, template, regex or character literal.
    Str,
    Number,
}

/// One token: comments and whitespace never become tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tok {
    pub kind: Kind,
    pub text: String,
    /// 1-based line where the token starts.
    pub line: u32,
    /// 1-based line where it ends; later than `line` for a multi-line string.
    pub end_line: u32,
}

impl Tok {
    /// Whether this is the punctuation `p`.
    pub fn is(&self, p: &str) -> bool {
        self.kind == Kind::Punct && self.text == p
    }

    /// Whether this is the identifier or keyword `word`.
    pub fn is_word(&self, word: &str) -> bool {
        self.kind == Kind::Ident && self.text == word
    }
}

/// A function the scanner found: its signature line and body extent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    /// The line of the signature's first token.
    pub start_line: u32,
    /// The last line of the body.
    pub end_line: u32,
    /// Body lines with code: not blank, not comment-only, not the signature.
    pub logical_lines: u32,
}

/// What a scan found in one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scanned {
    pub functions: Vec<Function>,
    /// Line ranges the scanner cannot judge, such as a `macro_rules!` body.
    pub opaque: Vec<(u32, u32)>,
}

/// For each bracket token, the index of its partner; `None` elsewhere.
pub fn pair_brackets(toks: &[Tok]) -> Result<Vec<Option<usize>>, Defer> {
    let mut pairs = vec![None; toks.len()];
    let mut open: Vec<usize> = Vec::new();
    for (i, t) in toks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.kind == Kind::Punct)
    {
        match t.text.as_str() {
            "(" | "[" | "{" => open.push(i),
            ")" | "]" | "}" => {
                let o = open.pop().ok_or(Defer("unbalanced-brackets"))?;
                if closer_of(&toks[o].text) != t.text {
                    return Err(Defer("mismatched-brackets"));
                }
                pairs[o] = Some(i);
                pairs[i] = Some(o);
            }
            _ => {}
        }
    }
    if open.is_empty() {
        Ok(pairs)
    } else {
        Err(Defer("unbalanced-brackets"))
    }
}

fn closer_of(open: &str) -> &'static str {
    match open {
        "(" => ")",
        "[" => "]",
        _ => "}",
    }
}

/// Every line some token in `toks` touches.
pub fn covered_lines(toks: &[Tok]) -> BTreeSet<u32> {
    toks.iter().flat_map(|t| t.line..=t.end_line).collect()
}

/// The logical lines of a brace body: lines the tokens strictly between
/// `open` and `close` cover, minus the line of the opening brace.
pub fn brace_body_lines(toks: &[Tok], open: usize, close: usize) -> u32 {
    let mut lines = covered_lines(&toks[open + 1..close]);
    lines.remove(&toks[open].line);
    lines.len() as u32
}

/// A function name fit for a daemon-written description.
pub fn display_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .take(64)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(kind: Kind, text: &str, line: u32) -> Tok {
        Tok {
            kind,
            text: text.into(),
            line,
            end_line: line,
        }
    }

    #[test]
    fn brackets_pair_and_mismatches_defer() {
        let toks: Vec<Tok> = ["{", "(", ")", "}"]
            .iter()
            .map(|p| tok(Kind::Punct, p, 1))
            .collect();
        let pairs = pair_brackets(&toks).unwrap();
        assert_eq!(pairs, vec![Some(3), Some(2), Some(1), Some(0)]);
        let bad = [tok(Kind::Punct, "(", 1), tok(Kind::Punct, "}", 1)];
        assert_eq!(pair_brackets(&bad), Err(Defer("mismatched-brackets")));
        assert!(pair_brackets(&bad[..1]).is_err());
    }

    #[test]
    fn body_lines_skip_the_brace_line_and_count_string_spans() {
        let mut s = tok(Kind::Str, "\"a\nb\"", 3);
        s.end_line = 4;
        let toks = vec![
            tok(Kind::Punct, "{", 1),
            tok(Kind::Ident, "x", 1),
            s,
            tok(Kind::Punct, "}", 6),
        ];
        assert_eq!(brace_body_lines(&toks, 0, 3), 2);
        assert_eq!(display_name("a<b>`c"), "abc");
    }
}
