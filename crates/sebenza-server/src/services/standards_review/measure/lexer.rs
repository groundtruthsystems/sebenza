//! A lexer for the brace languages, Rust and TS/JS. It only needs to tell
//! code from comments and to keep brackets inside strings, raw strings,
//! templates, regexes and character literals from counting. Anything it
//! cannot read with certainty, such as an unterminated string or an
//! ambiguous regex, is a [`Defer`].

use super::tokens::{Defer, Kind, Tok};

/// Which brace language is being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Rust,
    Js,
}

/// Tokens of `text`, without comments.
pub fn lex(text: &str, flavor: Flavor) -> Result<Vec<Tok>, Defer> {
    let mut lx = Lexer {
        chars: text.chars().collect(),
        i: 0,
        line: 1,
        flavor,
        toks: Vec::new(),
        depth: 0,
        templates: Vec::new(),
    };
    while lx.skip_space_and_comments()? {
        lx.token()?;
    }
    if !lx.templates.is_empty() {
        return Err(Defer("unterminated-template"));
    }
    Ok(lx.toks)
}

struct Lexer {
    chars: Vec<char>,
    i: usize,
    line: u32,
    flavor: Flavor,
    toks: Vec<Tok>,
    /// Open `{` count, so a template's `}` can be told from a block's.
    depth: usize,
    /// The brace depth at each open template interpolation.
    templates: Vec<usize>,
}

/// JS keywords after which `/` starts a regex rather than a division.
const REGEX_AFTER_WORDS: [&str; 15] = [
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "do",
    "else",
    "yield",
    "await",
    "extends",
];

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

    fn push(&mut self, kind: Kind, start: usize, line: u32) {
        let text: String = self.chars[start..self.i].iter().collect();
        self.toks.push(Tok {
            kind,
            text,
            line,
            end_line: self.line,
        });
    }

    /// Skip whitespace and comments; false at the end of input.
    fn skip_space_and_comments(&mut self) -> Result<bool, Defer> {
        loop {
            match (self.peek(0), self.peek(1)) {
                (None, _) => return Ok(false),
                (Some(c), _) if c.is_whitespace() => {
                    self.bump();
                }
                (Some('/'), Some('/')) => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        self.bump();
                    }
                }
                (Some('/'), Some('*')) => self.block_comment()?,
                _ => return Ok(true),
            }
        }
    }

    /// A block comment; Rust's nest.
    fn block_comment(&mut self) -> Result<(), Defer> {
        self.i += 2;
        let mut depth = 1;
        while depth > 0 {
            match (self.peek(0), self.peek(1)) {
                (None, _) => return Err(Defer("unterminated-comment")),
                (Some('*'), Some('/')) => {
                    self.i += 2;
                    depth -= 1;
                }
                (Some('/'), Some('*')) if self.flavor == Flavor::Rust => {
                    self.i += 2;
                    depth += 1;
                }
                _ => {
                    self.bump();
                }
            }
        }
        Ok(())
    }

    fn token(&mut self) -> Result<(), Defer> {
        let (start, line) = (self.i, self.line);
        let c = self.peek(0).ok_or(Defer("eof"))?;
        match c {
            '"' => self.quoted('"', self.flavor == Flavor::Rust)?,
            '\'' if self.flavor == Flavor::Js => self.quoted('\'', false)?,
            '\'' => return self.char_or_lifetime(),
            '`' if self.flavor == Flavor::Js => return self.template_chunk(),
            '/' if self.flavor == Flavor::Js && self.regex_allowed() => self.regex()?,
            '}' if self.templates.last() == Some(&self.depth) => {
                self.templates.pop();
                self.i += 1;
                return self.template_rest();
            }
            c if c.is_alphabetic() || c == '_' || c == '$' => return self.word(),
            c if c.is_ascii_digit() => {
                self.number();
                return Ok(());
            }
            _ => return self.punct(),
        }
        self.push(Kind::Str, start, line);
        Ok(())
    }

    /// A quoted string from its opening quote; JS strings cannot span lines.
    fn quoted(&mut self, quote: char, multiline: bool) -> Result<(), Defer> {
        self.bump();
        loop {
            match self.bump() {
                None => return Err(Defer("unterminated-string")),
                Some('\\') => {
                    self.bump();
                }
                Some('\n') if !multiline => return Err(Defer("unterminated-string")),
                Some(c) if c == quote => return Ok(()),
                Some(_) => {}
            }
        }
    }

    /// `'x'` and `'\n'` are characters; `'a` alone is a lifetime or label.
    fn char_or_lifetime(&mut self) -> Result<(), Defer> {
        let (start, line) = (self.i, self.line);
        if self.peek(1) == Some('\\') {
            self.i += 2;
            self.bump();
            while self.peek(0).is_some_and(|c| c != '\'' && c != '\n') {
                self.bump();
            }
            if self.bump() != Some('\'') {
                return Err(Defer("unterminated-char"));
            }
        } else if self.peek(2) == Some('\'') {
            self.i += 3;
        } else {
            self.i += 1;
            self.push(Kind::Punct, start, line);
            return Ok(());
        }
        self.push(Kind::Str, start, line);
        Ok(())
    }

    /// An identifier, or a Rust literal that starts like one (`r#"..."#`,
    /// `b"..."`, `b'x'`) or a raw identifier (`r#fn`).
    fn word(&mut self) -> Result<(), Defer> {
        let (start, line) = (self.i, self.line);
        while self
            .peek(0)
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$')
        {
            self.i += 1;
        }
        if self.flavor == Flavor::Rust {
            let word: String = self.chars[start..self.i].iter().collect();
            if self.rust_prefixed_literal(&word)? {
                self.push(Kind::Str, start, line);
                return Ok(());
            }
        }
        self.push(Kind::Ident, start, line);
        Ok(())
    }

    /// Consume a literal `word` prefixes; false when it prefixes none.
    fn rust_prefixed_literal(&mut self, word: &str) -> Result<bool, Defer> {
        let next = self.peek(0);
        match (word, next) {
            ("r" | "br" | "cr", Some('#')) if self.peek(1).is_some_and(is_word_char) => {
                self.i += 1;
                while self.peek(0).is_some_and(is_word_char) {
                    self.i += 1;
                }
                Ok(false)
            }
            ("r" | "br" | "cr", Some('"' | '#')) => self.raw_string().map(|_| true),
            ("b" | "c", Some('"')) => self.quoted('"', true).map(|_| true),
            ("b", Some('\'')) => {
                self.char_or_lifetime()?;
                let tok = self.toks.pop().ok_or(Defer("byte-char"))?;
                Ok(tok.kind == Kind::Str)
            }
            _ => Ok(false),
        }
    }

    /// A raw string from its `#`s or quote: no escapes, ends at `"###`.
    fn raw_string(&mut self) -> Result<(), Defer> {
        let mut hashes = 0;
        while self.peek(0) == Some('#') {
            hashes += 1;
            self.i += 1;
        }
        if self.bump() != Some('"') {
            return Err(Defer("bad-raw-string"));
        }
        loop {
            match self.bump() {
                None => return Err(Defer("unterminated-raw-string")),
                Some('"') if (0..hashes).all(|k| self.peek(k) == Some('#')) => {
                    self.i += hashes;
                    return Ok(());
                }
                Some(_) => {}
            }
        }
    }

    fn number(&mut self) {
        let (start, line) = (self.i, self.line);
        while let Some(c) = self.peek(0) {
            let fraction = c == '.' && self.peek(1).is_some_and(|d| d.is_ascii_digit());
            if !(is_word_char(c) || fraction) {
                break;
            }
            self.i += 1;
        }
        self.push(Kind::Number, start, line);
    }

    /// One punctuation token; `->`, `=>` and `::` stay whole.
    fn punct(&mut self) -> Result<(), Defer> {
        let (start, line) = (self.i, self.line);
        let pair: String = [self.peek(0), self.peek(1)].iter().flatten().collect();
        let width = if matches!(pair.as_str(), "->" | "=>" | "::") {
            2
        } else {
            1
        };
        match self.peek(0) {
            Some('{') => self.depth += 1,
            Some('}') => self.depth = self.depth.checked_sub(1).ok_or(Defer("unbalanced"))?,
            _ => {}
        }
        self.i += width;
        self.push(Kind::Punct, start, line);
        Ok(())
    }

    /// Whether `/` here opens a regex: only where a value may start.
    fn regex_allowed(&self) -> bool {
        if self.peek(1).is_some_and(|c| c == '/' || c == '*') {
            return false;
        }
        match self.toks.last() {
            None => true,
            Some(t) if t.kind == Kind::Ident => REGEX_AFTER_WORDS.contains(&t.text.as_str()),
            Some(t) if t.kind == Kind::Punct => !matches!(t.text.as_str(), ")" | "]" | "}"),
            Some(_) => false,
        }
    }

    /// A regex literal and its flags; a newline inside it defers.
    fn regex(&mut self) -> Result<(), Defer> {
        self.bump();
        let mut class = false;
        loop {
            match self.bump() {
                None | Some('\n') => return Err(Defer("ambiguous-regex")),
                Some('\\') => {
                    self.bump();
                }
                Some('[') => class = true,
                Some(']') => class = false,
                Some('/') if !class => break,
                Some(_) => {}
            }
        }
        while self.peek(0).is_some_and(is_word_char) {
            self.i += 1;
        }
        Ok(())
    }

    /// A template literal from its opening backtick.
    fn template_chunk(&mut self) -> Result<(), Defer> {
        self.i += 1;
        self.template_rest()
    }

    /// Template text up to the closing backtick or the next `${`.
    fn template_rest(&mut self) -> Result<(), Defer> {
        let (start, line) = (self.i, self.line);
        loop {
            match self.bump() {
                None => return Err(Defer("unterminated-template")),
                Some('\\') => {
                    self.bump();
                }
                Some('`') => break,
                Some('$') if self.peek(0) == Some('{') => {
                    self.i += 1;
                    self.templates.push(self.depth);
                    break;
                }
                Some(_) => {}
            }
        }
        self.push(Kind::Str, start, line);
        Ok(())
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str, flavor: Flavor) -> Vec<String> {
        lex(src, flavor)
            .unwrap()
            .into_iter()
            .map(|t| t.text)
            .collect()
    }

    #[test]
    fn rust_comments_strings_and_chars_hide_their_braces() {
        let src = "/* { /* nested } */ */ let s = \"{\\\"}\"; // }\nlet c = '{'; let l: &'a str = r#\"}\"#;";
        let t = texts(src, Flavor::Rust);
        assert!(!t.iter().any(|x| x == "{" || x == "}"), "{t:?}");
        assert!(t.contains(&"r#\"}\"#".to_string()), "{t:?}");
        assert!(t.contains(&"'{'".to_string()) && t.contains(&"'".to_string()));
    }

    #[test]
    fn rust_raw_identifiers_and_byte_literals() {
        let t = texts("let r#fn = b'}'; let x = br\"{\";", Flavor::Rust);
        assert_eq!(t[1], "r#fn");
        assert!(!t.iter().any(|x| x == "{" || x == "}"), "{t:?}");
    }

    #[test]
    fn js_templates_regexes_and_strings() {
        let src = "const a = `x ${ {k: 1}.k } }`; const r = /[}]\\//g; const s = '}';";
        let t = texts(src, Flavor::Js);
        let braces: Vec<_> = t.iter().filter(|x| *x == "{" || *x == "}").collect();
        assert_eq!(braces.len(), 2, "only the object literal's braces: {t:?}");
        assert_eq!(texts("x = a / b / c;", Flavor::Js).len(), 8);
    }

    #[test]
    fn unreadable_input_defers() {
        assert!(lex("let s = \"open", Flavor::Rust).is_err());
        assert!(lex("/* open", Flavor::Rust).is_err());
        assert!(lex("const s = 'a\nb';", Flavor::Js).is_err());
        assert!(lex("x = (/re\n/)", Flavor::Js).is_err());
        assert!(lex("`open ${", Flavor::Js).is_err());
    }
}
