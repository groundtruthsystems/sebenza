//! Rust functions: `fn <name>` with a body. A `fn` with no body (a trait
//! method's declaration) or used as a type (`fn(u8)`) is not one. Functions
//! inside a `macro_rules!` body are templates, so that body is opaque.

use super::lexer::{Flavor, lex};
use super::tokens::{Defer, Function, Kind, Scanned, Tok, brace_body_lines, pair_brackets};

/// The functions of a Rust source file.
pub fn scan(text: &str) -> Result<Scanned, Defer> {
    let toks = lex(text, Flavor::Rust)?;
    let pairs = pair_brackets(&toks)?;
    let macros = macro_rules_ranges(&toks, &pairs);
    let in_macro = |i: usize| macros.iter().any(|(a, b)| (*a..=*b).contains(&i));
    let mut functions = Vec::new();
    for i in 0..toks.len().saturating_sub(1) {
        let named = toks[i].is_word("fn") && toks[i + 1].kind == Kind::Ident;
        if named && !in_macro(i) {
            functions.extend(function_at(&toks, &pairs, i)?);
        }
    }
    let opaque = macros
        .iter()
        .map(|(a, b)| (toks[*a].line, toks[*b].end_line))
        .collect();
    Ok(Scanned { functions, opaque })
}

/// The function whose `fn` keyword is token `i`, if it has a body.
fn function_at(toks: &[Tok], pairs: &[Option<usize>], i: usize) -> Result<Option<Function>, Defer> {
    let Some(open) = body_open(toks, pairs, i + 2)? else {
        return Ok(None);
    };
    let close = pairs[open].ok_or(Defer("unpaired-body"))?;
    Ok(Some(Function {
        name: toks[i + 1].text.clone(),
        start_line: toks[i].line,
        end_line: toks[close].line,
        logical_lines: brace_body_lines(toks, open, close),
    }))
}

/// The `{` opening the body of the signature starting at `from`; `None`
/// when a `;` ends it first.
fn body_open(toks: &[Tok], pairs: &[Option<usize>], from: usize) -> Result<Option<usize>, Defer> {
    let mut angle: i32 = 0;
    let mut j = from;
    while j < toks.len() {
        let t = &toks[j];
        match t.text.as_str() {
            "(" | "[" if t.kind == Kind::Punct => j = pairs[j].ok_or(Defer("unpaired"))?,
            "<" if t.kind == Kind::Punct => angle += 1,
            ">" if t.kind == Kind::Punct => angle -= 1,
            ";" if t.kind == Kind::Punct && angle == 0 => return Ok(None),
            "{" if t.kind == Kind::Punct && angle == 0 => return Ok(Some(j)),
            "{" if t.kind == Kind::Punct => j = pairs[j].ok_or(Defer("unpaired"))?,
            _ => {}
        }
        if angle < 0 {
            return Err(Defer("unbalanced-generics"));
        }
        j += 1;
    }
    Ok(None)
}

/// Token ranges of every `macro_rules! name { ... }` (or `(...)`/`[...]`).
fn macro_rules_ranges(toks: &[Tok], pairs: &[Option<usize>]) -> Vec<(usize, usize)> {
    (0..toks.len().saturating_sub(3))
        .filter(|&i| toks[i].is_word("macro_rules") && toks[i + 1].is("!"))
        .filter_map(|i| Some((i, pairs[i + 3]?)))
        .collect()
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
    fn a_body_counts_code_lines_only() {
        let src = "/// Doc.\nfn a(x: u8) -> u8 {\n    // why\n\n    let y = x;\n    y\n}\n";
        assert_eq!(lengths(src), vec![("a".into(), 2)]);
        let f = &scan(src).unwrap().functions[0];
        assert_eq!((f.start_line, f.end_line), (2, 7));
    }

    #[test]
    fn declarations_and_fn_types_are_not_functions() {
        let src = "trait T { fn d(&self); fn e(&self) -> u8 { 1 } }\ntype F = fn(u8) -> u8;\n";
        assert_eq!(lengths(src), vec![("e".into(), 0)]);
    }

    #[test]
    fn macro_rules_bodies_are_opaque() {
        let src =
            "macro_rules! m {\n    () => { fn fake() { x(); } };\n}\nfn real() {\n    m!();\n}\n";
        let s = scan(src).unwrap();
        assert_eq!(s.functions.len(), 1);
        assert_eq!(s.opaque, vec![(1, 3)]);
    }
}
