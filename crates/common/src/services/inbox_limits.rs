//! Caps and rate limits on inbox ingress.
//!
//! Every worktree agent holds the control token (T-01), so nothing stops one
//! from looping on `sebenza-agentctl request`. Each request will later cost a
//! model turn, so floods are bounded here, before anything is appended: body
//! caps, a per-caller sliding window, and a ceiling on open requests per item.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Largest comment or request body accepted, in bytes.
pub const MAX_BODY_BYTES: usize = 16 * 1024;
/// Longest request title accepted, in characters.
pub const MAX_TITLE_CHARS: usize = 200;
/// Largest raw `/api/runtime/events` payload carrying an inbox event, in bytes.
pub const MAX_INGRESS_BYTES: usize = 64 * 1024;
/// Open (unresolved) requests one item may hold before new ones are refused.
pub const MAX_OPEN_REQUESTS: usize = 20;

/// `max` hits per `window`, per caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    pub max: usize,
    pub window: Duration,
}

/// Every bound the inbox enforces on comment and request ingress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboxLimits {
    pub max_body_bytes: usize,
    pub max_title_chars: usize,
    pub max_open_requests: usize,
    pub comments: RateLimit,
    pub requests: RateLimit,
}

impl Default for InboxLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: MAX_BODY_BYTES,
            max_title_chars: MAX_TITLE_CHARS,
            max_open_requests: MAX_OPEN_REQUESTS,
            comments: RateLimit {
                max: 30,
                window: Duration::from_secs(60),
            },
            requests: RateLimit {
                max: 10,
                window: Duration::from_secs(60),
            },
        }
    }
}

/// A sliding-window counter keyed by caller. In memory: a restart forgets it,
/// which is acceptable for a flood guard on a loopback daemon.
pub struct RateLimiter {
    limit: RateLimit,
    hits: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl RateLimiter {
    pub fn new(limit: RateLimit) -> Self {
        Self {
            limit,
            hits: Mutex::new(HashMap::new()),
        }
    }

    /// Record a hit for `key` now. False when the caller is over its limit,
    /// in which case nothing is recorded.
    pub fn allow(&self, key: &str) -> bool {
        self.allow_at(key, Instant::now())
    }

    /// As [`Self::allow`], at an explicit instant.
    pub fn allow_at(&self, key: &str, now: Instant) -> bool {
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        let recent = hits.entry(key.to_string()).or_default();
        while recent
            .front()
            .is_some_and(|&at| now.saturating_duration_since(at) >= self.limit.window)
        {
            recent.pop_front();
        }
        if recent.len() >= self.limit.max {
            return false;
        }
        recent.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter(max: usize, secs: u64) -> RateLimiter {
        RateLimiter::new(RateLimit {
            max,
            window: Duration::from_secs(secs),
        })
    }

    // TS-13: a caller over its rate is refused.
    #[test]
    fn a_caller_over_its_limit_is_refused() {
        let l = limiter(3, 60);
        let t = Instant::now();
        assert!(l.allow_at("wt:a", t));
        assert!(l.allow_at("wt:a", t));
        assert!(l.allow_at("wt:a", t));
        assert!(!l.allow_at("wt:a", t), "the fourth hit in the window");
    }

    #[test]
    fn callers_are_counted_separately() {
        let l = limiter(1, 60);
        let t = Instant::now();
        assert!(l.allow_at("wt:a", t));
        assert!(
            l.allow_at("wt:b", t),
            "one flooding worktree must not starve another"
        );
        assert!(!l.allow_at("wt:a", t));
    }

    #[test]
    fn the_window_slides() {
        let l = limiter(1, 60);
        let t = Instant::now();
        assert!(l.allow_at("wt:a", t));
        assert!(!l.allow_at("wt:a", t + Duration::from_secs(59)));
        assert!(l.allow_at("wt:a", t + Duration::from_secs(61)));
    }

    #[test]
    fn a_refused_hit_does_not_extend_the_block() {
        let l = limiter(1, 60);
        let t = Instant::now();
        assert!(l.allow_at("wt:a", t));
        // Hammering while blocked must not push the window out.
        for s in 1..60 {
            assert!(!l.allow_at("wt:a", t + Duration::from_secs(s)));
        }
        assert!(l.allow_at("wt:a", t + Duration::from_secs(61)));
    }
}
