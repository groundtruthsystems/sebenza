//! Edit-boundary globs (FR-20, FR-21): the paths a task's commits may change.
//!
//! A glob matches a whole repository-relative path. `**` spans any number of
//! directories, including none; `*` and `?` stay inside one path segment. A
//! glob ending in `/` covers everything under that directory. An empty
//! boundary covers nothing.

/// Whether `path` is inside any glob of `globs`.
pub fn in_boundary(globs: &[String], path: &str) -> bool {
    globs.iter().any(|g| glob_matches(g, path))
}

/// Whether `glob` matches `path`.
pub fn glob_matches(glob: &str, path: &str) -> bool {
    todo!()
}

/// Whether `glob` is a relative, normalised path pattern.
pub fn valid_glob(glob: &str) -> bool {
    todo!()
}

fn segments_match(globs: &[&str], parts: &[&str]) -> bool {
    match globs.split_first() {
        None => parts.is_empty(),
        Some((&"**", rest)) => (0..=parts.len()).any(|i| segments_match(rest, &parts[i..])),
        Some((glob, rest)) => parts.split_first().is_some_and(|(part, tail)| {
            segment_matches(glob.as_bytes(), part.as_bytes()) && segments_match(rest, tail)
        }),
    }
}

fn segment_matches(glob: &[u8], text: &[u8]) -> bool {
    match glob.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|i| segment_matches(rest, &text[i..])),
        Some((b'?', rest)) => !text.is_empty() && segment_matches(rest, &text[1..]),
        Some((c, rest)) => text.first() == Some(c) && segment_matches(rest, &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_paths_match_only_themselves() {
        assert!(glob_matches("src/a.rs", "src/a.rs"));
        assert!(!glob_matches("src/a.rs", "src/a.rsx"));
        assert!(!glob_matches("src/a.rs", "lib/src/a.rs"));
    }

    #[test]
    fn stars_stay_in_a_segment_and_double_stars_span_directories() {
        assert!(glob_matches("src/*.rs", "src/a.rs"));
        assert!(!glob_matches("src/*.rs", "src/x/a.rs"));
        assert!(glob_matches("src/**/*.rs", "src/a.rs"));
        assert!(glob_matches("src/**/*.rs", "src/x/y/a.rs"));
        assert!(glob_matches("**", "any/where.txt"));
        assert!(glob_matches("src/?.rs", "src/a.rs"));
        assert!(!glob_matches("src/?.rs", "src/ab.rs"));
    }

    #[test]
    fn a_trailing_slash_covers_the_directory() {
        assert!(glob_matches("crates/common/", "crates/common/src/lib.rs"));
        assert!(!glob_matches("crates/common/", "crates/commonx/lib.rs"));
    }

    #[test]
    fn an_empty_boundary_covers_nothing() {
        assert!(!in_boundary(&[], "src/a.rs"));
        assert!(in_boundary(
            &["docs/".into(), "src/a.rs".into()],
            "src/a.rs"
        ));
    }

    #[test]
    fn escaping_or_absolute_globs_are_invalid() {
        for bad in ["", "/etc/x", "../x", "a/../b", "a//b", "./a", "a\\b"] {
            assert!(!valid_glob(bad), "{bad}");
        }
        for good in ["src/", "src/**/*.rs", "Cargo.toml", "**"] {
            assert!(valid_glob(good), "{good}");
        }
    }
}
