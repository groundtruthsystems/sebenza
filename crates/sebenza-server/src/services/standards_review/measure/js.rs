//! TS/JS functions: a block that follows `=>`, or a `(...)` parameter list
//! (with an optional return type) after `function`, a function or method
//! name, or a generic parameter list. `if (...) {` and the other statement
//! keywords are not functions. A body followed straight by another `{` means
//! a type literal was mistaken for a body, so the file defers.

use super::lexer::{Flavor, lex};
use super::tokens::{Defer, Function, Kind, Scanned, Tok, brace_body_lines, pair_brackets};

/// Words before `(` that make it a statement or expression, not a function.
const NOT_CALLEES: [&str; 16] = [
    "if", "for", "while", "switch", "catch", "with", "return", "typeof", "await", "yield", "in",
    "of", "new", "delete", "void", "throw",
];

/// Words that end a backward search for a return type.
const STOP_WORDS: [&str; 10] = [
    "else",
    "return",
    "do",
    "try",
    "finally",
    "class",
    "interface",
    "enum",
    "namespace",
    "extends",
];

/// How far back a return type annotation is searched for, in tokens.
const MAX_RETURN_TYPE_TOKENS: usize = 200;

/// The functions of a TS or JS source file.
pub fn scan(text: &str) -> Result<Scanned, Defer> {
    let toks = lex(text, Flavor::Js)?;
    let pairs = pair_brackets(&toks)?;
    let mut functions = Vec::new();
    for open in (0..toks.len()).filter(|&i| toks[i].is("{")) {
        let Some((name, start_line)) = head_of(&toks, &pairs, open) else {
            continue;
        };
        let close = pairs[open].ok_or(Defer("unpaired-body"))?;
        if toks.get(close + 1).is_some_and(|t| t.is("{")) {
            return Err(Defer("ambiguous-type-literal"));
        }
        functions.push(Function {
            name,
            start_line,
            end_line: toks[close].line,
            logical_lines: brace_body_lines(&toks, open, close),
        });
    }
    Ok(Scanned {
        functions,
        opaque: Vec::new(),
    })
}

/// The name and first line of the function whose body opens at `open`.
fn head_of(toks: &[Tok], pairs: &[Option<usize>], open: usize) -> Option<(String, u32)> {
    let prev = open.checked_sub(1)?;
    if toks[prev].is("=>") {
        return Some(arrow_head(toks, pairs, prev));
    }
    let rparen = if toks[prev].is(")") {
        prev
    } else {
        return_type_paren(toks, pairs, prev)?
    };
    let lparen = pairs[rparen]?;
    callee(toks, lparen).map(|(name, line)| (name, line.min(toks[lparen].line)))
}

/// An arrow function's name, if assigned, and its first line.
fn arrow_head(toks: &[Tok], pairs: &[Option<usize>], arrow: usize) -> (String, u32) {
    let params_end = arrow.saturating_sub(1);
    let first = if toks[params_end].is(")") {
        pairs[params_end].unwrap_or(params_end)
    } else {
        return_type_paren(toks, pairs, params_end)
            .and_then(|r| pairs[r])
            .unwrap_or(params_end)
    };
    let name = first
        .checked_sub(2)
        .filter(|&n| toks[n].kind == Kind::Ident && (toks[n + 1].is("=") || toks[n + 1].is(":")))
        .map_or_else(|| "anonymous".to_string(), |n| toks[n].text.clone());
    (name, toks[first].line)
}

/// Walk back from `from` over a return type to the `)` before its `:`.
fn return_type_paren(toks: &[Tok], pairs: &[Option<usize>], from: usize) -> Option<usize> {
    let (mut j, mut angle) = (from, 0i32);
    for _ in 0..MAX_RETURN_TYPE_TOKENS {
        let t = &toks[j];
        match t.text.as_str() {
            ")" | "]" | "}" if t.kind == Kind::Punct => j = pairs[j]?,
            ">" if t.kind == Kind::Punct => angle += 1,
            "<" if t.kind == Kind::Punct => angle -= 1,
            ":" if t.kind == Kind::Punct && angle == 0 => {
                return toks
                    .get(j.checked_sub(1)?)
                    .filter(|p| p.is(")"))
                    .map(|_| j - 1);
            }
            ";" | "{" | "=" | "," if t.kind == Kind::Punct && angle == 0 => return None,
            w if t.kind == Kind::Ident && STOP_WORDS.contains(&w) => return None,
            _ => {}
        }
        j = j.checked_sub(1)?;
    }
    None
}

/// What precedes the parameter list at `lparen`: `function`, a name, or a
/// name with generic parameters.
fn callee(toks: &[Tok], lparen: usize) -> Option<(String, u32)> {
    let mut k = lparen.checked_sub(1)?;
    if toks[k].is(">") {
        k = matching_angle(toks, k)?.checked_sub(1)?;
    }
    let t = &toks[k];
    if t.kind != Kind::Ident || NOT_CALLEES.contains(&t.text.as_str()) {
        return None;
    }
    let keyword = k.checked_sub(1).filter(|&f| toks[f].is_word("function"));
    let line = keyword.map_or(t.line, |f| toks[f].line);
    Some((t.text.clone(), line))
}

/// The `<` that opens the generic list closing at `close`.
fn matching_angle(toks: &[Tok], close: usize) -> Option<usize> {
    let mut depth = 0;
    for j in (0..=close).rev() {
        if toks[j].is(">") {
            depth += 1;
        } else if toks[j].is("<") {
            depth -= 1;
            if depth == 0 {
                return Some(j);
            }
        }
    }
    None
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
    fn functions_methods_and_arrows_are_found() {
        let src = "function a(x) {\n  return x;\n}\nclass C {\n  m(): void {\n    go();\n    stop();\n  }\n}\nconst f = (y: number) => {\n  y++;\n};\n";
        assert_eq!(
            lengths(src),
            vec![("a".into(), 1), ("m".into(), 2), ("f".into(), 1)]
        );
    }

    #[test]
    fn statements_and_classes_are_not_functions() {
        let src = "if (a) {\n  b();\n}\nfor (;;) {\n  c();\n}\nclass D extends E {\n}\nconst o = { k: 1 };\n";
        assert!(lengths(src).is_empty());
    }

    #[test]
    fn return_types_and_generics_are_skipped() {
        let src =
            "function g<T extends {a: number}>(x: T): Promise<{b: string}> {\n  return h(x);\n}\n";
        assert_eq!(lengths(src), vec![("g".into(), 1)]);
    }
}
