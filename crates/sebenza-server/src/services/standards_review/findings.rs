//! The finding lifecycle: the transition table from data-architecture.md
//! and the normalised-text hash findings are matched by.
//!
//! | From | To | Actor |
//! |---|---|---|
//! | open | needs-operator-decision | daemon |
//! | needs-operator-decision | open | operator |
//! | needs-operator-decision | resolved | operator |
//! | open | resolved | daemon |
//! | open | resolved-out-of-scope | daemon |
//! | open | rule-withdrawn | daemon |
//!
//! Every other move, including any move out of a terminal status, is
//! refused. A regression after resolution is a new finding, never a reopen.

use common::domain::review_records::FindingStatus;
use common::domain::solution::sha256_hex;

/// Who asks for a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Daemon,
    Operator,
}

/// The allowed transitions, exactly as the table above.
pub const TRANSITIONS: [(FindingStatus, FindingStatus, Actor); 6] = {
    use FindingStatus::*;
    [
        (Open, NeedsOperatorDecision, Actor::Daemon),
        (NeedsOperatorDecision, Open, Actor::Operator),
        (NeedsOperatorDecision, Resolved, Actor::Operator),
        (Open, Resolved, Actor::Daemon),
        (Open, ResolvedOutOfScope, Actor::Daemon),
        (Open, RuleWithdrawn, Actor::Daemon),
    ]
};

/// A transition the table does not allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("finding transition {} -> {} by {actor:?} is not allowed", from.as_str(), to.as_str())]
pub struct TransitionRefused {
    pub from: FindingStatus,
    pub to: FindingStatus,
    pub actor: Actor,
}

/// Allow `from -> to` by `actor` only when the table lists it.
pub fn check_transition(
    from: FindingStatus,
    to: FindingStatus,
    actor: Actor,
) -> Result<(), TransitionRefused> {
    todo!()
}

/// Lowercase alphanumeric words separated by single spaces: punctuation,
/// case and spacing differences do not change a finding's identity.
pub fn normalise(text: &str) -> String {
    todo!()
}

/// The SHA-256 of [`normalise`]d `text`.
pub fn text_hash(text: &str) -> String {
    sha256_hex(&normalise(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use FindingStatus::*;

    #[test]
    fn ts61_every_listed_transition_succeeds_and_every_other_is_refused() {
        for from in FindingStatus::ALL {
            for to in FindingStatus::ALL {
                for actor in [Actor::Daemon, Actor::Operator] {
                    let listed = TRANSITIONS.contains(&(from, to, actor));
                    let got = check_transition(from, to, actor);
                    assert_eq!(got.is_ok(), listed, "{from:?} -> {to:?} by {actor:?}");
                    if !listed {
                        assert_eq!(got, Err(TransitionRefused { from, to, actor }));
                    }
                }
            }
        }
    }

    #[test]
    fn ts61_terminal_statuses_never_move() {
        for from in FindingStatus::ALL.into_iter().filter(|s| s.is_terminal()) {
            for to in FindingStatus::ALL {
                assert!(check_transition(from, to, Actor::Daemon).is_err());
                assert!(check_transition(from, to, Actor::Operator).is_err());
            }
        }
    }

    #[test]
    fn ts61_the_daemon_cannot_take_operator_transitions() {
        assert!(check_transition(NeedsOperatorDecision, Resolved, Actor::Daemon).is_err());
        assert!(check_transition(Open, Resolved, Actor::Operator).is_err());
    }

    #[test]
    fn normalising_ignores_case_punctuation_and_spacing() {
        assert_eq!(
            normalise("  Function `parse` is 42 lines;\n too LONG! "),
            "function parse is 42 lines too long"
        );
        assert_eq!(text_hash("Too long."), text_hash("too   long"));
        assert_ne!(text_hash("too long"), text_hash("too short"));
    }
}
