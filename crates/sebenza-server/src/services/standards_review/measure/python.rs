//! Python functions: `def` (or `async def`) at the start of a logical line.
//! The body is every following logical line indented deeper than the `def`.
//! A leading docstring counts as a comment. Indentation that mixes tabs and
//! spaces, or a signature with no closing `:`, defers.

use super::tokens::{Defer, Function, Kind, Scanned, Tok, covered_lines, pair_brackets};

/// The start of one logical line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Logical {
    first_tok: usize,
    indent: usize,
}

/// The functions of a Python source file.
pub fn scan(text: &str) -> Result<Scanned, Defer> {
    let (toks, lines) = lex(text)?;
    let pairs = pair_brackets(&toks)?;
    let mut functions = Vec::new();
    for (li, l) in lines.iter().enumerate() {
        let def = match (toks.get(l.first_tok), toks.get(l.first_tok + 1)) {
            (Some(a), Some(_)) if a.is_word("def") => l.first_tok,
            (Some(a), Some(b)) if a.is_word("async") && b.is_word("def") => l.first_tok + 1,
            _ => continue,
        };
        functions.push(function_at(&toks, &pairs, &lines, li, def)?);
    }
    Ok(Scanned {
        functions,
        opaque: Vec::new(),
    })
}

fn function_at(
    toks: &[Tok],
    pairs: &[Option<usize>],
    lines: &[Logical],
    li: usize,
    def: usize,
) -> Result<Function, Defer> {
    let name = toks.get(def + 1).filter(|t| t.kind == Kind::Ident);
    let name = name.ok_or(Defer("unnamed-def"))?;
    let colon = signature_colon(toks, pairs, def + 2).ok_or(Defer("no-signature-colon"))?;
    let start_line = toks[lines[li].first_tok].line;
    let one_liner = toks
        .get(colon + 1)
        .is_some_and(|t| t.line == toks[colon].line);
    let body = if one_liner {
        Vec::new()
    } else {
        body_lines(toks, lines, li)?
    };
    Ok(Function {
        name: name.text.clone(),
        start_line,
        end_line: body.last().map_or(toks[colon].line, |(_, end)| *end),
        logical_lines: body.iter().map(|(n, _)| n).sum(),
    })
}

/// The `:` ending a signature that starts at `from`, past brackets.
fn signature_colon(toks: &[Tok], pairs: &[Option<usize>], from: usize) -> Option<usize> {
    let mut j = from;
    while j < toks.len() {
        let t = &toks[j];
        if t.is("(") || t.is("[") || t.is("{") {
            j = pairs[j]?;
        } else if t.is(":") {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// Per body statement after the `def` at logical line `li`: its code lines
/// and last line. A leading docstring contributes no lines.
fn body_lines(toks: &[Tok], lines: &[Logical], li: usize) -> Result<Vec<(u32, u32)>, Defer> {
    let indent = lines[li].indent;
    let body: Vec<usize> = (li + 1..lines.len())
        .take_while(|&k| lines[k].indent > indent)
        .collect();
    if body.is_empty() {
        return Err(Defer("missing-body"));
    }
    let statement = |k: usize| {
        let end = lines.get(k + 1).map_or(toks.len(), |n| n.first_tok);
        &toks[lines[k].first_tok..end]
    };
    Ok(body
        .iter()
        .enumerate()
        .map(|(n, &k)| statement_lines(statement(k), n == 0))
        .collect())
}

/// One body statement's code lines and last line; a leading docstring
/// (`first` and a lone string) has none.
fn statement_lines(toks: &[Tok], first: bool) -> (u32, u32) {
    let docstring = first && toks.len() == 1 && toks[0].kind == Kind::Str;
    let count = if docstring {
        0
    } else {
        covered_lines(toks).len() as u32
    };
    (count, toks.last().map_or(0, |t| t.end_line))
}

/// Tokens and logical-line starts of `text`.
fn lex(text: &str) -> Result<(Vec<Tok>, Vec<Logical>), Defer> {
    let mut lx = Lexer {
        chars: text.chars().collect(),
        i: 0,
        line: 1,
        depth: 0,
        toks: Vec::new(),
        lines: Vec::new(),
        indent_chars: (false, false),
    };
    while lx.i < lx.chars.len() {
        lx.physical_line()?;
    }
    if lx.depth != 0 {
        return Err(Defer("unbalanced-brackets"));
    }
    Ok((lx.toks, lx.lines))
}

struct Lexer {
    chars: Vec<char>,
    i: usize,
    line: u32,
    /// Open brackets: inside them a newline does not end the line.
    depth: i32,
    toks: Vec<Tok>,
    lines: Vec<Logical>,
    /// Whether any indentation used (tabs, spaces).
    indent_chars: (bool, bool),
}

impl Lexer {
    fn peek(&self, k: usize) -> Option<char> {
        self.chars.get(self.i + k).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.i += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    /// One logical line starting at a fresh physical line.
    fn physical_line(&mut self) -> Result<(), Defer> {
        let indent = self.indent()?;
        let mut first = true;
        while let Some(c) = self.peek(0) {
            if let Some(ends) = self.newline() {
                if ends {
                    return Ok(());
                }
            } else if c == '#' {
                self.skip_comment();
            } else if c.is_whitespace() {
                self.bump();
            } else {
                if first && self.depth == 0 {
                    let first_tok = self.toks.len();
                    self.lines.push(Logical { first_tok, indent });
                }
                first = false;
                self.token()?;
            }
        }
        Ok(())
    }

    /// Consume a newline or a backslash continuation, if one is next; true
    /// when it ends the logical line.
    fn newline(&mut self) -> Option<bool> {
        match (self.peek(0), self.peek(1)) {
            (Some('\n'), _) => {
                self.bump();
                Some(self.depth == 0)
            }
            (Some('\\'), Some('\n')) => {
                self.i += 2;
                self.line += 1;
                Some(false)
            }
            _ => None,
        }
    }

    /// Leading whitespace width; tabs and spaces may not both indent.
    fn indent(&mut self) -> Result<usize, Defer> {
        let mut width = 0;
        while let Some(c @ (' ' | '\t')) = self.peek(0) {
            if c == '\t' {
                self.indent_chars.0 = true;
            } else {
                self.indent_chars.1 = true;
            }
            width += 1;
            self.i += 1;
        }
        let blank = matches!(self.peek(0), None | Some('\n' | '#'));
        if !blank && self.indent_chars == (true, true) {
            return Err(Defer("mixed-indentation"));
        }
        Ok(width)
    }

    fn skip_comment(&mut self) {
        while self.peek(0).is_some_and(|c| c != '\n') {
            self.i += 1;
        }
    }

    fn token(&mut self) -> Result<(), Defer> {
        let (start, line) = (self.i, self.line);
        let kind = self.token_kind()?;
        let text: String = self.chars[start..self.i].iter().collect();
        self.toks.push(Tok {
            kind,
            text,
            line,
            end_line: self.line,
        });
        Ok(())
    }

    /// Consume one token and say what it is.
    fn token_kind(&mut self) -> Result<Kind, Defer> {
        let c = self.peek(0).ok_or(Defer("eof"))?;
        if let Some(prefix) = self.string_prefix() {
            self.i += prefix;
            self.string()?;
            Ok(Kind::Str)
        } else if c.is_alphanumeric() || c == '_' {
            self.word();
            Ok(if c.is_ascii_digit() {
                Kind::Number
            } else {
                Kind::Ident
            })
        } else {
            self.punct();
            Ok(Kind::Punct)
        }
    }

    /// The length of a string prefix (`r`, `b`, `f`, `rb`, ...) when a
    /// string literal starts here.
    fn string_prefix(&self) -> Option<usize> {
        (0..=2).find(|&n| {
            let letters = (0..n).all(|k| {
                self.peek(k)
                    .is_some_and(|c| matches!(c.to_ascii_lowercase(), 'r' | 'b' | 'u' | 'f'))
            });
            letters && matches!(self.peek(n), Some('"' | '\''))
        })
    }

    /// A string from its opening quote, single or triple.
    fn string(&mut self) -> Result<(), Defer> {
        let quote = self.bump().ok_or(Defer("eof"))?;
        let triple = self.peek(0) == Some(quote) && self.peek(1) == Some(quote);
        if triple {
            self.i += 2;
        }
        loop {
            match self.bump() {
                None => return Err(Defer("unterminated-string")),
                Some('\\') => {
                    self.bump();
                }
                Some('\n') if !triple => return Err(Defer("unterminated-string")),
                Some(c) if c == quote && !triple => return Ok(()),
                Some(c) if c == quote && self.peek(0) == Some(c) && self.peek(1) == Some(c) => {
                    self.i += 2;
                    return Ok(());
                }
                Some(_) => {}
            }
        }
    }

    fn word(&mut self) {
        while self
            .peek(0)
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            self.i += 1;
        }
    }

    fn punct(&mut self) {
        match self.peek(0) {
            Some('(' | '[' | '{') => self.depth += 1,
            Some(')' | ']' | '}') => self.depth -= 1,
            _ => {}
        }
        let arrow = self.peek(0) == Some('-') && self.peek(1) == Some('>');
        self.i += if arrow { 2 } else { 1 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lengths(src: &str) -> Vec<(String, u32)> {
        scan(src)
            .unwrap()
            .functions
            .into_iter()
            .map(|f| (f.name, f.logical_lines))
            .collect()
    }

    #[test]
    fn a_body_excludes_its_docstring_comments_and_blanks() {
        let src = "def a(x):\n    \"\"\"Doc.\n    More.\"\"\"\n    # why\n\n    y = x\n    return y\n\nz = 1\n";
        assert_eq!(lengths(src), vec![("a".into(), 2)]);
        let f = &scan(src).unwrap().functions[0];
        assert_eq!((f.start_line, f.end_line), (1, 7));
    }

    #[test]
    fn nested_defs_one_liners_and_multiline_signatures() {
        let src = "class C:\n    async def m(\n        self,\n    ) -> int:\n        def inner():\n            return 1\n        return inner()\n\n    def one(self): return 2\n";
        assert_eq!(
            lengths(src),
            vec![("m".into(), 3), ("inner".into(), 1), ("one".into(), 0)]
        );
    }

    #[test]
    fn mixed_indentation_and_open_strings_defer() {
        assert!(scan("def a():\n\tx = 1\n        y = 2\n").is_err());
        assert!(scan("def a():\n    s = 'open\n").is_err());
    }
}
