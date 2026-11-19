//! Run tokens (FR-11): the single-use credential of one review run.
//!
//! A token is 32 random bytes in hex behind an `rt_` prefix. The daemon hands
//! it to the run's agent once and appends only its SHA-256 hash to the
//! review log; it is bound to one run id and kind, dies with the lease and
//! is consumed by the first valid submit. It never reaches a log line.

use common::domain::solution::sha256_hex;

/// The prefix every run token carries, so it cannot pass for another class.
pub const TOKEN_PREFIX: &str = "rt_";

/// A freshly minted run token. `Debug` never shows it.
#[derive(Clone, PartialEq, Eq)]
pub struct RunToken(String);

impl std::fmt::Debug for RunToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RunToken(<redacted>)")
    }
}

impl RunToken {
    /// A new random token.
    pub fn mint() -> RunToken {
        RunToken(format!(
            "{TOKEN_PREFIX}{}",
            common::util::id::random_hex(32)
        ))
    }

    /// The token itself, for delivery to the run's agent only.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The hash the log stores.
    pub fn hash(&self) -> String {
        hash_token(&self.0)
    }
}

/// The SHA-256 hex of a presented token.
pub fn hash_token(token: &str) -> String {
    sha256_hex(token)
}

/// Whether `token` has a run token's shape: the prefix and 64 hex digits.
pub fn looks_like_run_token(token: &str) -> bool {
    token
        .strip_prefix(TOKEN_PREFIX)
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_prefixed_and_hashed_as_sha256() {
        let a = RunToken::mint();
        let b = RunToken::mint();
        assert_ne!(a, b);
        assert!(looks_like_run_token(a.expose()));
        assert_eq!(a.hash(), sha256_hex(a.expose()));
        assert_eq!(a.hash().len(), 64);
        assert_ne!(a.hash(), a.expose());
    }

    #[test]
    fn debug_never_shows_the_token() {
        let t = RunToken::mint();
        assert!(!format!("{t:?}").contains(&t.expose()[3..]));
    }

    #[test]
    fn other_credential_shapes_are_not_run_tokens() {
        assert!(!looks_like_run_token(&format!("op_{}", "a".repeat(64))));
        assert!(!looks_like_run_token("route-test-token"));
        assert!(!looks_like_run_token(&format!("rt_{}", "z".repeat(64))));
    }
}
