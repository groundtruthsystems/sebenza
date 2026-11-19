//! The finding matcher (FR-17): does a reported finding continue a prior
//! open finding, or is it new?
//!
//! A prior finding's path is first carried to the current head through the
//! git rename map (its anchor SHA to the job's head, `-M`). A claim then
//! matches, in order:
//! 1. a prior with the same rule, the same (renamed) path and the same
//!    normalised-text hash;
//! 2. a prior with the same rule and text hash anywhere, so a symbol moved
//!    to another file keeps its finding.
//!
//! Each prior matches at most one claim. Only open priors are offered:
//! a claim like a resolved finding is a regression and a new finding.

use super::git_pinned::{Git, GitError};
use std::collections::BTreeMap;

/// Paths renamed between two commits, old to new.
pub type RenameMap = BTreeMap<String, String>;

/// The renames from `from_sha` to `to_sha`, by git's `-M` detection.
pub fn rename_map(git: &Git, from_sha: &str, to_sha: &str) -> Result<RenameMap, GitError> {
    if from_sha == to_sha {
        return Ok(RenameMap::new());
    }
    let out = git.run(&["diff", "--name-status", "-z", "-M", from_sha, to_sha])?;
    Ok(parse_renames(&out))
}

/// `--name-status -z`: a status, then one path, or two for `R` and `C`.
fn parse_renames(out: &[u8]) -> RenameMap {
    let fields: Vec<String> = out
        .split(|b| *b == 0)
        .map(|f| String::from_utf8_lossy(f).to_string())
        .collect();
    let mut map = RenameMap::new();
    let mut i = 0;
    while i < fields.len() {
        let status = fields[i].chars().next();
        let paths = if matches!(status, Some('R' | 'C')) {
            2
        } else {
            1
        };
        if status == Some('R')
            && let (Some(old), Some(new)) = (fields.get(i + 1), fields.get(i + 2))
        {
            map.insert(old.clone(), new.clone());
        }
        i += 1 + paths;
    }
    map
}

/// A prior open finding as the matcher sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorKey {
    pub rule_id: String,
    /// The anchor path carried to the current head.
    pub path: String,
    pub text_hash: String,
}

/// A reported finding as the matcher sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimKey {
    pub rule_id: String,
    pub path: String,
    pub text_hash: String,
}

/// For each claim, the index of the prior it continues, if any.
pub fn match_claims(claims: &[ClaimKey], priors: &[PriorKey]) -> Vec<Option<usize>> {
    let mut taken = vec![false; priors.len()];
    let mut matched = vec![None; claims.len()];
    let same_place = |c: &ClaimKey, p: &PriorKey| same_text(c, p) && c.path == p.path;
    for pass in [
        &same_place as &dyn Fn(&ClaimKey, &PriorKey) -> bool,
        &same_text,
    ] {
        for (c, slot) in claims.iter().zip(matched.iter_mut()) {
            if slot.is_some() {
                continue;
            }
            *slot = (0..priors.len()).find(|&i| !taken[i] && pass(c, &priors[i]));
            if let Some(i) = *slot {
                taken[i] = true;
            }
        }
    }
    matched
}

fn same_text(c: &ClaimKey, p: &PriorKey) -> bool {
    c.rule_id == p.rule_id && c.text_hash == p.text_hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::{TestRepo, git};

    fn prior(rule: &str, path: &str, hash: &str) -> PriorKey {
        PriorKey {
            rule_id: rule.into(),
            path: path.into(),
            text_hash: hash.into(),
        }
    }

    fn claim(rule: &str, path: &str, hash: &str) -> ClaimKey {
        ClaimKey {
            rule_id: rule.into(),
            path: path.into(),
            text_hash: hash.into(),
        }
    }

    #[test]
    fn ts32_the_rename_map_carries_a_path_between_heads() {
        let r = TestRepo::new("matcher-rename");
        let body = "fn long() {\n".to_string() + &"    step();\n".repeat(30) + "}\n";
        let before = r.commit_on_feat("src/old_name.rs", &body);
        git(&r.worktree, &["mv", "src/old_name.rs", "src/new_name.rs"]);
        git(&r.worktree, &["commit", "-q", "-m", "rename"]);
        let after = r.head();
        let g = Git::open(&r.repo).unwrap();
        let map = rename_map(&g, &before, &after).unwrap();
        assert_eq!(
            map.get("src/old_name.rs").map(String::as_str),
            Some("src/new_name.rs")
        );
        assert!(rename_map(&g, &after, &after).unwrap().is_empty());
    }

    #[test]
    fn ts32_a_renamed_path_with_the_same_text_keeps_its_finding() {
        let priors = [prior("long", "src/new_name.rs", "h1")];
        let got = match_claims(&[claim("long", "src/new_name.rs", "h1")], &priors);
        assert_eq!(got, vec![Some(0)]);
    }

    #[test]
    fn ts33_a_moved_symbol_follows_by_rule_and_text_hash() {
        let priors = [prior("long", "src/a.rs", "h1")];
        let got = match_claims(&[claim("long", "src/elsewhere.rs", "h1")], &priors);
        assert_eq!(got, vec![Some(0)]);
    }

    #[test]
    fn the_same_path_wins_over_a_text_match_elsewhere() {
        let priors = [
            prior("long", "src/a.rs", "h1"),
            prior("long", "src/b.rs", "h1"),
        ];
        let got = match_claims(&[claim("long", "src/b.rs", "h1")], &priors);
        assert_eq!(got, vec![Some(1)]);
    }

    #[test]
    fn a_different_rule_or_text_is_a_new_finding_and_each_prior_matches_once() {
        let priors = [prior("long", "src/a.rs", "h1")];
        let claims = [
            claim("naming", "src/a.rs", "h1"),
            claim("long", "src/a.rs", "h2"),
            claim("long", "src/a.rs", "h1"),
            claim("long", "src/a.rs", "h1"),
        ];
        assert_eq!(
            match_claims(&claims, &priors),
            vec![None, None, Some(0), None]
        );
    }
}
