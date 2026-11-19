//! Batched blob reads through `git cat-file`: sizes first, so an oversized
//! blob is never read, then contents.

use super::git_pinned::{Git, GitError, RunLimits};
use std::collections::BTreeMap;

/// The size in bytes of each blob in `oids` that exists; others are absent.
pub fn blob_sizes(git: &Git, oids: &[String]) -> Result<BTreeMap<String, u64>, GitError> {
    if oids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let out = git.run_limited(&["cat-file", "--batch-check"], with_input(oids, None))?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(parse_blob_header)
        .collect())
}

/// The contents of each blob in `oids`, which must exist and together stay
/// within `max_total` bytes.
pub fn read_blobs(
    git: &Git,
    oids: &[String],
    max_total: usize,
) -> Result<BTreeMap<String, Vec<u8>>, GitError> {
    if oids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let framing = oids.len() * 100;
    let out = git.run_limited(
        &["cat-file", "--batch"],
        with_input(oids, Some(max_total + framing)),
    )?;
    parse_batch(&out)
}

fn with_input(oids: &[String], max_output: Option<usize>) -> RunLimits {
    let mut input = oids.join("\n");
    input.push('\n');
    RunLimits {
        input: Some(input.into_bytes()),
        max_output,
        ..RunLimits::default()
    }
}

/// `<oid> blob <size>`; other object types and `missing` lines yield `None`.
fn parse_blob_header(line: &str) -> Option<(String, u64)> {
    let mut f = line.split(' ');
    let (oid, kind, size) = (f.next()?, f.next()?, f.next()?);
    (kind == "blob").then_some(())?;
    Some((oid.to_string(), size.parse().ok()?))
}

/// Split `cat-file --batch` output: a header line, `size` bytes, a newline.
fn parse_batch(mut out: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, GitError> {
    let bad = || GitError::Failed("malformed cat-file output".to_string());
    let mut blobs = BTreeMap::new();
    while !out.is_empty() {
        let nl = out.iter().position(|b| *b == b'\n').ok_or_else(bad)?;
        let header = String::from_utf8_lossy(&out[..nl]).to_string();
        let (oid, size) = parse_blob_header(&header).ok_or_else(bad)?;
        let body = out.get(nl + 1..nl + 1 + size as usize).ok_or_else(bad)?;
        blobs.insert(oid, body.to_vec());
        out = out.get(nl + 2 + size as usize..).unwrap_or_default();
    }
    Ok(blobs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::review_test_repo::{TestRepo, git};

    #[test]
    fn sizes_and_contents_come_from_objects() {
        let r = TestRepo::new("objects");
        let wf = git(&r.repo, &["rev-parse", "main:.ai/sebenza/workflow.md"]);
        let general = git(
            &r.repo,
            &["rev-parse", "main:.ai/sebenza/code_styleguides/general.md"],
        );
        let g = Git::open(&r.repo).unwrap();
        let oids = vec![wf.clone(), general.clone(), "0".repeat(40)];
        let sizes = blob_sizes(&g, &oids).unwrap();
        assert_eq!(sizes.get(&wf), Some(&11));
        assert_eq!(sizes.len(), 2);
        let blobs = read_blobs(&g, &[wf.clone(), general.clone()], 1024).unwrap();
        assert_eq!(blobs[&wf], b"# Workflow\n");
        assert_eq!(blobs[&general], b"# General\n");
    }

    #[test]
    fn a_read_over_its_total_cap_fails() {
        let r = TestRepo::new("objects-cap");
        r.commit_on_main("big.md", &"x".repeat(4096));
        let big = git(&r.repo, &["rev-parse", "main:big.md"]);
        let g = Git::open(&r.repo).unwrap();
        assert_eq!(read_blobs(&g, &[big], 1024), Err(GitError::OutputTooLarge));
    }
}
