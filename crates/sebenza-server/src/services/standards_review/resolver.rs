//! The snapshot and input resolver (FR-06..FR-08, FR-10, NFR-03): from
//! pinned head, merge-base and target SHAs, everything a review run reads.
//!
//! It holds no lock and reads only git objects, so a claim handler must pin
//! and resolve before taking any daemon lock. A git failure or timeout fails
//! the attempt ([`ResolveError`]); a problem with the inputs themselves is a
//! [`Blocker`] in an otherwise complete [`ReviewInputs`], which makes the
//! job non-passing.

use super::git_pinned::{Git, GitError};
use super::manifest::{RuleManifest, build_manifest};
use super::reasons::Blocker;
use super::scope::{
    DiffFile, ReviewLimits, ScopeCommit, SnapshotPins, StandardsChange, first_parent_commits,
    scope_diff, standards_change,
};
use super::standards_docs::resolve_standards;
use super::standards_rev::{StandardsEntry, rev_of};
use serde::Serialize;

/// Why a resolution produced nothing; the run attempt fails.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    #[error("git: {0}")]
    Git(#[from] GitError),
}

/// Everything a review run reads, derived from pinned objects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewInputs {
    pub pins: SnapshotPins,
    pub standards_rev: String,
    /// The revision inputs, with (path, `missing`) sentinels as `None`.
    pub standards: Vec<StandardsEntry>,
    pub manifest: RuleManifest,
    /// First-parent commits base..head, oldest first, for the scope proof.
    pub commits: Vec<ScopeCommit>,
    pub files: Vec<DiffFile>,
    /// Set when the track edits its standards; rules still come from the
    /// merge-base.
    pub standards_change: Option<StandardsChange>,
    /// Empty unless the job is non-passing whatever the review finds.
    pub blockers: Vec<Blocker>,
}

impl ReviewInputs {
    /// Whether the inputs alone already rule out a passing verdict.
    pub fn is_blocked(&self) -> bool {
        !self.blockers.is_empty()
    }
}

/// Pin `head_rev`, `target_rev` and their merge-base to commit SHAs.
pub fn pin(git: &Git, head_rev: &str, target_rev: &str) -> Result<SnapshotPins, ResolveError> {
    let head_sha = git.commit_sha(head_rev)?;
    let target_sha = git.commit_sha(target_rev)?;
    let base_sha = git.text(&["merge-base", &head_sha, &target_sha])?;
    Ok(SnapshotPins {
        head_sha,
        base_sha,
        target_sha,
    })
}

/// Resolve the inputs at `pins`: standards and rules from the merge-base,
/// scope from base→head.
pub fn resolve(
    git: &Git,
    pins: &SnapshotPins,
    limits: &ReviewLimits,
) -> Result<ReviewInputs, ResolveError> {
    let standards = resolve_standards(git, &pins.base_sha, limits.max_blob_bytes)?;
    let standards_rev = rev_of(&standards.entries);
    let (manifest, rule_blockers) = build_manifest(&standards_rev, &standards.docs);
    let commits = first_parent_commits(git, pins)?;
    let diff = scope_diff(git, pins, limits)?;
    let mut blockers: Vec<Blocker> = standards
        .blockers
        .into_iter()
        .chain(rule_blockers)
        .chain(diff.blockers)
        .collect();
    blockers.sort();
    Ok(ReviewInputs {
        pins: pins.clone(),
        standards_rev,
        standards: standards.entries,
        manifest,
        commits,
        standards_change: standards_change(&diff.files),
        files: diff.files,
        blockers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::{TestRepo, write};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    const STYLE: &str = ".ai/sebenza/code_styleguides/general.md";

    fn inputs(r: &TestRepo) -> ReviewInputs {
        let git = Git::open(&r.repo).unwrap();
        let pins = pin(&git, "feat", "main").unwrap();
        resolve(&git, &pins, &ReviewLimits::default()).unwrap()
    }

    fn rule_ids(i: &ReviewInputs) -> Vec<&str> {
        i.manifest
            .rules
            .iter()
            .map(|r| r.rule_id.as_str())
            .collect()
    }

    fn with_rules(r: &TestRepo) {
        r.commit_on_main(
            STYLE,
            "# General\n- [rule:short category=long-methods max_lines=20] Short.\n",
        );
        r.merge_main_into_feat();
    }

    #[test]
    fn pins_are_full_commit_shas() {
        let r = TestRepo::new("resolve-pins");
        let git = Git::open(&r.worktree).unwrap();
        let p = pin(&git, "feat", "main").unwrap();
        assert_eq!(p.head_sha, r.head());
        assert_eq!(p.target_sha, r.main_head());
        assert_eq!(p.base_sha, r.main_head());
        assert!(pin(&git, "no-such-branch", "main").is_err());
    }

    #[test]
    fn a_clean_track_resolves_unblocked_with_its_manifest_and_hunks() {
        let r = TestRepo::new("resolve-ok");
        with_rules(&r);
        r.commit_on_feat("lib.rs", "fn a() {}\n");
        let i = inputs(&r);
        assert!(!i.is_blocked(), "{:?}", i.blockers);
        assert_eq!(rule_ids(&i), vec!["short"]);
        assert_eq!(i.manifest.rules[0].source_doc, STYLE);
        assert_eq!(i.manifest.rules[0].line, 2);
        assert_eq!(i.manifest.standards_rev, i.standards_rev);
        assert_eq!(i.standards_rev, rev_of(&i.standards));
        assert!(i.files.iter().any(|f| f.path() == "lib.rs"));
        assert_eq!(
            i.commits.last().map(|c| c.sha.as_str()),
            Some(i.pins.head_sha.as_str())
        );
        assert_eq!(i.standards_change, None);
    }

    #[test]
    fn ts07_working_tree_edits_after_claim_do_not_reach_the_manifest() {
        let r = TestRepo::new("resolve-ts07");
        with_rules(&r);
        let git = Git::open(&r.worktree).unwrap();
        let pins = pin(&git, "feat", "main").unwrap();
        let planted = "- [rule:planted category=naming] Planted.\n";
        write(&r.worktree, STYLE, planted);
        write(&r.repo, STYLE, planted);
        let i = resolve(&git, &pins, &ReviewLimits::default()).unwrap();
        assert_eq!(rule_ids(&i), vec!["short"]);
    }

    #[test]
    fn ts17_a_track_editing_its_standards_keeps_the_merge_base_rules() {
        let r = TestRepo::new("resolve-ts17");
        with_rules(&r);
        r.commit_on_feat(
            STYLE,
            "# General\n- [rule:lenient category=long-methods max_lines=500] Long.\n",
        );
        let i = inputs(&r);
        assert_eq!(rule_ids(&i), vec!["short"]);
        assert_eq!(i.manifest.rules[0].max_lines, Some(20));
        assert_eq!(
            i.standards_change,
            Some(StandardsChange {
                paths: vec![STYLE.to_string()]
            })
        );
    }

    #[test]
    fn ts09_an_escaping_entry_blocks_the_job() {
        let r = TestRepo::new("resolve-ts09");
        r.commit_on_main(".ai/sebenza/index.md", "- [E](../../src.rs)\n");
        r.merge_main_into_feat();
        let i = inputs(&r);
        assert!(i.is_blocked());
        let json = serde_json::to_value(&i.blockers).unwrap();
        assert_eq!(json[0]["code"], "standards-unavailable");
        assert_eq!(json[0]["reason"], "outside-workspace");
    }

    #[test]
    fn ts11_a_missing_doc_puts_its_sentinel_in_the_job() {
        let r = TestRepo::new("resolve-ts11");
        r.commit_on_main(".ai/sebenza/index.md", "- [G](./product-guidelines.md)\n");
        r.merge_main_into_feat();
        let i = inputs(&r);
        assert!(i.is_blocked());
        assert!(i.standards.contains(&StandardsEntry {
            path: ".ai/sebenza/product-guidelines.md".into(),
            blob: None
        }));
    }

    #[test]
    fn ts51_caps_block_the_job() {
        let r = TestRepo::new("resolve-ts51");
        r.commit_on_feat("big.rs", &"let x = 1;\n".repeat(500));
        let git = Git::open(&r.repo).unwrap();
        let pins = pin(&git, "feat", "main").unwrap();
        let limits = ReviewLimits {
            max_diff_bytes: 512,
            ..ReviewLimits::default()
        };
        let i = resolve(&git, &pins, &limits).unwrap();
        let json = serde_json::to_value(&i.blockers).unwrap();
        assert_eq!(json[0]["code"], "cap-exceeded");
        assert_eq!(json[0]["reason"], "diff-too-large");
    }

    #[test]
    fn ts57_a_stalled_git_fails_the_attempt_and_holds_no_lock() {
        let r = TestRepo::new("resolve-ts57");
        let stall = r.base.join("stalled-git");
        write(&r.base, "stalled-git", "#!/bin/sh\nexec sleep 30\n");
        std::process::Command::new("chmod")
            .arg("+x")
            .arg(&stall)
            .status()
            .unwrap();
        let real = Git::open(&r.repo).unwrap();
        let pins = pin(&real, "feat", "main").unwrap();
        let stalled = real
            .clone()
            .with_program(&stall)
            .with_timeout(Duration::from_millis(300));
        let daemon_state = Arc::new(Mutex::new(0u32));
        let started = Instant::now();
        let hung = std::thread::spawn(move || resolve(&stalled, &pins, &ReviewLimits::default()));
        std::thread::sleep(Duration::from_millis(50));
        *daemon_state.lock().unwrap() += 1;
        let other = inputs(&r);
        assert!(!other.pins.head_sha.is_empty());
        let err = hung.join().unwrap().unwrap_err();
        assert_eq!(err, ResolveError::Git(GitError::Timeout));
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(*daemon_state.lock().unwrap(), 1);
    }
}
