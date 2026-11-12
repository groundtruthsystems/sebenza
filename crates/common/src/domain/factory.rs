//! The `factory:` block of `~/.ai/sebenza.yaml`: which agent and model play
//! each software-factory role, and the reviewer independence rule (BR-08).
//!
//! ```yaml
//! factory:
//!   solution:
//!     coordinator: { agent: claude, model: claude-opus-5-5 }
//!     designer:    { agent: claude, model: claude-opus-5-5 }
//!     reviewer:    { agent: codex }
//!   project:
//!     reviewer:    { agent: claude, model: claude-fable-5-2 }
//! ```
//!
//! A per-item `factory:` override in the draft frontmatter wins over this
//! block, role by role ([`FactoryConfig::resolve`]).

use super::solution::{AgentProfile, FactoryOverrides};
use serde::{Deserialize, Serialize};

/// The agents a factory role may run on.
pub const FACTORY_AGENTS: [&str; 3] = ["claude", "codex", "grok"];

/// Agents whose read-only enforcement is not yet verified by the conformance
/// tests (technical-architecture.md). They may coordinate, but never review.
pub const UNVERIFIED_REVIEWER_AGENTS: [&str; 1] = ["grok"];

/// The agent a role runs on when nothing configures it.
pub const DEFAULT_ROLE_AGENT: &str = "claude";

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct SolutionProfiles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator: Option<AgentProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub designer: Option<AgentProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<AgentProfile>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ProjectProfiles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<AgentProfile>,
}

/// The global role profiles. Round limits are constants
/// ([`super::solution::MAX_REVIEW_ROUNDS`]), not configuration.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct FactoryConfig {
    #[serde(default)]
    pub solution: SolutionProfiles,
    #[serde(default)]
    pub project: ProjectProfiles,
}

/// Every role's profile for one item, after overrides. Reviewers stay
/// optional: an unset reviewer is refused at run, not guessed.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveProfiles {
    pub coordinator: AgentProfile,
    pub designer: AgentProfile,
    pub solution_reviewer: Option<AgentProfile>,
    pub project_reviewer: Option<AgentProfile>,
}

impl FactoryConfig {
    /// Apply an item's overrides role by role over this global block.
    pub fn resolve(&self, overrides: Option<&FactoryOverrides>) -> EffectiveProfiles {
        let o = overrides.cloned().unwrap_or_default();
        let fallback = || AgentProfile::new(DEFAULT_ROLE_AGENT, None);
        EffectiveProfiles {
            coordinator: o
                .coordinator
                .or_else(|| self.solution.coordinator.clone())
                .unwrap_or_else(fallback),
            designer: o
                .designer
                .or_else(|| self.solution.designer.clone())
                .unwrap_or_else(fallback),
            solution_reviewer: o
                .reviewer
                .solution
                .or_else(|| self.solution.reviewer.clone()),
            project_reviewer: o.reviewer.project.or_else(|| self.project.reviewer.clone()),
        }
    }

    /// The check run when this block is saved: every profile names a known
    /// agent, and every configured reviewer is independent where that can be
    /// judged now. See [`validate_profiles`].
    pub fn validate(&self) -> Result<(), FactoryConfigError> {
        validate_profiles(&self.resolve(None))
    }
}

/// Validate the profiles a save would leave in effect: known agents, and an
/// independent solution reviewer against the designer. The project reviewer is
/// checked for what it is on its own (not lightweight, not unverified); its
/// designer is the worktree's agent, so the full rule runs at review time
/// ([`check_independence`]).
pub fn validate_profiles(p: &EffectiveProfiles) -> Result<(), FactoryConfigError> {
    for (role, profile) in [
        ("coordinator", Some(&p.coordinator)),
        ("designer", Some(&p.designer)),
        ("solution reviewer", p.solution_reviewer.as_ref()),
        ("project reviewer", p.project_reviewer.as_ref()),
    ] {
        if let Some(profile) = profile {
            check_known_agent(role, profile)?;
        }
    }
    if let Some(r) = &p.solution_reviewer {
        check_independence(&p.designer, r).map_err(|e| FactoryConfigError::NotIndependent {
            level: "solution",
            reason: e,
        })?;
    }
    if let Some(r) = &p.project_reviewer {
        check_reviewer_alone(r).map_err(|e| FactoryConfigError::NotIndependent {
            level: "project",
            reason: e,
        })?;
    }
    Ok(())
}

fn check_known_agent(role: &str, p: &AgentProfile) -> Result<(), FactoryConfigError> {
    if FACTORY_AGENTS.contains(&p.agent.as_str()) {
        Ok(())
    } else {
        Err(FactoryConfigError::UnknownAgent {
            role: role.to_string(),
            agent: p.agent.clone(),
        })
    }
}

/// A `factory:` block or override the daemon refuses.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FactoryConfigError {
    #[error("factory: {0}")]
    Invalid(String),
    #[error("factory: {role} agent {agent:?} is not one of claude, codex, grok")]
    UnknownAgent { role: String, agent: String },
    /// 422 `not_independent`.
    #[error("factory: the {level} reviewer is not independent: {reason}")]
    NotIndependent {
        level: &'static str,
        reason: IndependenceError,
    },
}

impl FactoryConfigError {
    /// The API error code: `not_independent` for a reviewer refusal.
    pub fn code(&self) -> &'static str {
        match self {
            FactoryConfigError::NotIndependent { .. } => "not_independent",
            _ => "invalid_factory_config",
        }
    }
}

/// Why a reviewer may not review a designer's work (BR-08). Each says what to
/// change.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IndependenceError {
    #[error("no reviewer is configured")]
    NoReviewer,
    #[error("{model} is a lightweight model, which never reviews")]
    Lightweight { model: String },
    #[error(
        "{agent} cannot review: its read-only enforcement is unverified, so it may only coordinate"
    )]
    UnverifiedAgent { agent: String },
    #[error(
        "the reviewer is {agent} like the designer, so both models must be set explicitly and differ"
    )]
    ModelUnset { agent: String },
    #[error("the reviewer uses the designer's own agent and model ({agent} {model})")]
    SameModel { agent: String, model: String },
    #[error("{reviewer} is less capable than the designer's {designer}")]
    LessCapable { reviewer: String, designer: String },
    #[error(
        "the capability of {model} is unknown, so it cannot be shown to be at least as capable as the designer's model; pick a different agent or a known model"
    )]
    UnknownCapability { model: String },
}

/// How capable a model is, as far as independence cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelTier {
    /// Never reviews (e.g. Haiku).
    Lightweight,
    Standard,
    Advanced,
    Frontier,
}

/// The capability table. Lightweight markers are recognised for any agent;
/// tiers above that are known for Claude's families. Anything else is
/// `None`, which the same-agent rule treats as unprovable.
pub fn model_tier(model: &str) -> Option<ModelTier> {
    let m = model.to_ascii_lowercase();
    const LIGHT: [&str; 5] = ["haiku", "mini", "nano", "flash", "lite"];
    if LIGHT.iter().any(|w| m.contains(w)) {
        return Some(ModelTier::Lightweight);
    }
    if m.contains("fable") {
        Some(ModelTier::Frontier)
    } else if m.contains("opus") {
        Some(ModelTier::Advanced)
    } else if m.contains("sonnet") {
        Some(ModelTier::Standard)
    } else {
        None
    }
}

/// The checks that need no designer: a lightweight model or an unverified
/// agent never reviews.
pub fn check_reviewer_alone(reviewer: &AgentProfile) -> Result<(), IndependenceError> {
    if UNVERIFIED_REVIEWER_AGENTS.contains(&reviewer.agent.as_str()) {
        return Err(IndependenceError::UnverifiedAgent {
            agent: reviewer.agent.clone(),
        });
    }
    if let Some(model) = &reviewer.model {
        if model_tier(model) == Some(ModelTier::Lightweight) {
            return Err(IndependenceError::Lightweight {
                model: model.clone(),
            });
        }
    }
    Ok(())
}

/// BR-08: the reviewer is independent of the designer when it runs on a
/// different agent, or on the same agent with both models set explicitly,
/// different, and the reviewer's at least as capable. A lightweight model or
/// an unverified agent never reviews.
pub fn check_independence(
    designer: &AgentProfile,
    reviewer: &AgentProfile,
) -> Result<(), IndependenceError> {
    check_reviewer_alone(reviewer)?;
    if !reviewer.agent.eq_ignore_ascii_case(&designer.agent) {
        return Ok(());
    }
    let (Some(rm), Some(dm)) = (&reviewer.model, &designer.model) else {
        return Err(IndependenceError::ModelUnset {
            agent: reviewer.agent.clone(),
        });
    };
    if rm.eq_ignore_ascii_case(dm) {
        return Err(IndependenceError::SameModel {
            agent: reviewer.agent.clone(),
            model: rm.clone(),
        });
    }
    let Some(rt) = model_tier(rm) else {
        return Err(IndependenceError::UnknownCapability { model: rm.clone() });
    };
    let Some(dt) = model_tier(dm) else {
        return Err(IndependenceError::UnknownCapability { model: dm.clone() });
    };
    if rt < dt {
        return Err(IndependenceError::LessCapable {
            reviewer: rm.clone(),
            designer: dm.clone(),
        });
    }
    Ok(())
}

/// The reviewer to run for a review, or why none may run: unset, or not
/// independent of `designer`.
pub fn reviewer_for(
    designer: &AgentProfile,
    reviewer: Option<&AgentProfile>,
) -> Result<AgentProfile, IndependenceError> {
    let reviewer = reviewer.ok_or(IndependenceError::NoReviewer)?;
    check_independence(designer, reviewer)?;
    Ok(reviewer.clone())
}

/// Parse a `factory:` block. `None` is the empty default (every role on
/// claude, no reviewers). Unknown keys and agents are errors; independence is
/// checked by [`FactoryConfig::validate`].
pub fn parse_factory_config(
    raw: Option<&serde_yaml::Value>,
) -> Result<FactoryConfig, FactoryConfigError> {
    let Some(raw) = raw else {
        return Ok(FactoryConfig::default());
    };
    if raw.is_null() {
        return Ok(FactoryConfig::default());
    }
    let config: FactoryConfig = serde_yaml::from_value(raw.clone())
        .map_err(|e| FactoryConfigError::Invalid(e.to_string()))?;
    // `AgentProfile` is lenient (it also reads draft frontmatter); the global
    // block is strict, so a typo is reported rather than ignored.
    for level in ["solution", "project"] {
        let roles = raw.get(level).and_then(|v| v.as_mapping());
        for (role, profile) in roles.into_iter().flatten() {
            for key in profile.as_mapping().into_iter().flat_map(|m| m.keys()) {
                let key = key.as_str().unwrap_or_default();
                if !matches!(key, "agent" | "model") {
                    return Err(FactoryConfigError::Invalid(format!(
                        "{level}.{}: unknown key {key:?}",
                        role.as_str().unwrap_or_default()
                    )));
                }
            }
        }
    }
    let p = config.resolve(None);
    for (role, profile) in [
        ("solution.coordinator", config.solution.coordinator.as_ref()),
        ("solution.designer", config.solution.designer.as_ref()),
        ("solution.reviewer", p.solution_reviewer.as_ref()),
        ("project.reviewer", p.project_reviewer.as_ref()),
    ] {
        if let Some(profile) = profile {
            check_known_agent(role, profile)?;
        }
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::solution::ReviewerProfiles;

    fn p(agent: &str, model: Option<&str>) -> AgentProfile {
        AgentProfile::new(agent, model)
    }

    // TS-19: claude opus designer; same-agent reviewers need a more or equally
    // capable, explicit, non-lightweight model.
    #[test]
    fn independence_follows_br_08() {
        let designer = p("claude", Some("claude-opus-5-5"));
        let refused = |r: AgentProfile| check_independence(&designer, &r).unwrap_err();

        assert!(matches!(
            refused(p("claude", Some("claude-sonnet-5-5"))),
            IndependenceError::LessCapable { .. }
        ));
        assert!(matches!(
            refused(p("claude", Some("claude-haiku-5-5"))),
            IndependenceError::Lightweight { .. }
        ));
        assert!(matches!(
            refused(p("claude", None)),
            IndependenceError::ModelUnset { .. }
        ));
        assert!(matches!(
            refused(p("claude", Some("claude-opus-5-5"))),
            IndependenceError::SameModel { .. }
        ));
        assert!(matches!(
            refused(p("CLAUDE", Some("Claude-Opus-5-5"))),
            IndependenceError::SameModel { .. }
        ));

        assert_eq!(check_independence(&designer, &p("codex", None)), Ok(()));
        assert_eq!(
            check_independence(&designer, &p("claude", Some("claude-fable-5-2"))),
            Ok(())
        );
        assert_eq!(
            check_independence(&designer, &p("claude", Some("opus-4-1"))),
            Ok(()),
            "another model of the same tier is at least as capable"
        );
    }

    // TS-18: reviewer equal to the designer (same agent, same or unset model).
    #[test]
    fn a_reviewer_equal_to_the_designer_is_refused() {
        for (d, r) in [
            (p("codex", None), p("codex", None)),
            (p("codex", Some("gpt-5")), p("codex", Some("gpt-5"))),
            (p("claude", Some("opus")), p("claude", None)),
            (p("claude", None), p("claude", Some("opus"))),
        ] {
            let e = check_independence(&d, &r).unwrap_err();
            assert!(
                matches!(
                    e,
                    IndependenceError::ModelUnset { .. } | IndependenceError::SameModel { .. }
                ),
                "{d:?} vs {r:?}: {e:?}"
            );
        }
    }

    #[test]
    fn lightweight_and_unverified_reviewers_never_review() {
        let designer = p("claude", Some("opus"));
        assert!(matches!(
            check_independence(&designer, &p("codex", Some("gpt-5-mini"))),
            Err(IndependenceError::Lightweight { .. })
        ));
        let e = check_independence(&designer, &p("grok", None)).unwrap_err();
        assert!(matches!(e, IndependenceError::UnverifiedAgent { .. }));
        assert!(
            e.to_string().contains("unverified"),
            "the reason says why: {e}"
        );
    }

    #[test]
    fn unknown_capability_is_refused_for_the_same_agent_only() {
        let d = p("codex", Some("gpt-5"));
        assert!(matches!(
            check_independence(&d, &p("codex", Some("gpt-5.5"))),
            Err(IndependenceError::UnknownCapability { .. })
        ));
        assert_eq!(check_independence(&d, &p("claude", Some("sonnet"))), Ok(()));
    }

    #[test]
    fn every_refusal_has_a_reason() {
        let e = check_independence(&p("claude", Some("opus")), &p("claude", Some("sonnet")))
            .unwrap_err();
        assert_eq!(
            e.to_string(),
            "sonnet is less capable than the designer's opus"
        );
        let err = FactoryConfigError::NotIndependent {
            level: "solution",
            reason: e,
        };
        assert_eq!(err.code(), "not_independent");
        assert!(
            err.to_string()
                .contains("solution reviewer is not independent")
        );
        assert_eq!(
            reviewer_for(&p("claude", None), None),
            Err(IndependenceError::NoReviewer)
        );
    }

    fn global() -> FactoryConfig {
        parse_factory_config(Some(
            &serde_yaml::from_str(
                "solution:\n  coordinator: {agent: codex}\n  designer: {agent: claude, model: opus}\n  reviewer: {agent: codex}\nproject:\n  reviewer: {agent: claude, model: fable}\n",
            )
            .unwrap(),
        ))
        .unwrap()
    }

    #[test]
    fn the_block_parses_and_defaults_to_claude_with_no_reviewers() {
        let cfg = global();
        assert_eq!(cfg.solution.coordinator, Some(p("codex", None)));
        assert_eq!(cfg.project.reviewer, Some(p("claude", Some("fable"))));
        assert_eq!(cfg.validate(), Ok(()));

        let none = parse_factory_config(None).unwrap().resolve(None);
        assert_eq!(none.coordinator, p("claude", None));
        assert_eq!(none.designer, p("claude", None));
        assert_eq!(none.solution_reviewer, None);
        assert_eq!(none.project_reviewer, None);
    }

    #[test]
    fn bad_blocks_are_refused() {
        let parse = |y: &str| parse_factory_config(Some(&serde_yaml::from_str(y).unwrap()));
        assert!(matches!(
            parse("limits: {rounds: 5}"),
            Err(FactoryConfigError::Invalid(_))
        ));
        assert!(matches!(
            parse("solution: {reviewer: {agent: opencode}}"),
            Err(FactoryConfigError::UnknownAgent { .. })
        ));
        assert!(matches!(
            parse("solution: {reviewer: {agent: codex, temperature: 1}}"),
            Err(FactoryConfigError::Invalid(_))
        ));
        // Shape is fine, independence is not: refused when saved.
        let cfg = parse("solution: {designer: {agent: codex}, reviewer: {agent: codex}}").unwrap();
        assert!(matches!(
            cfg.validate(),
            Err(FactoryConfigError::NotIndependent {
                level: "solution",
                ..
            })
        ));
        let cfg = parse("project: {reviewer: {agent: claude, model: claude-haiku-5-5}}").unwrap();
        assert!(matches!(
            cfg.validate(),
            Err(FactoryConfigError::NotIndependent {
                level: "project",
                ..
            })
        ));
    }

    // FR-08: per-item overrides win role by role.
    #[test]
    fn item_overrides_take_precedence() {
        let o = FactoryOverrides {
            designer: Some(p("codex", None)),
            reviewer: ReviewerProfiles {
                solution: Some(p("claude", Some("fable"))),
                project: None,
            },
            ..Default::default()
        };
        let eff = global().resolve(Some(&o));
        assert_eq!(eff.coordinator, p("codex", None), "from the global block");
        assert_eq!(eff.designer, p("codex", None), "overridden");
        assert_eq!(eff.solution_reviewer, Some(p("claude", Some("fable"))));
        assert_eq!(eff.project_reviewer, Some(p("claude", Some("fable"))));
        assert_eq!(validate_profiles(&eff), Ok(()));

        // An override that makes the reviewer equal the designer is refused.
        let clash = FactoryOverrides {
            designer: Some(p("codex", None)),
            ..Default::default()
        };
        let eff = global().resolve(Some(&clash));
        assert!(matches!(
            validate_profiles(&eff),
            Err(FactoryConfigError::NotIndependent { .. })
        ));
    }

    #[test]
    fn the_tier_table_orders_known_models() {
        assert_eq!(model_tier("claude-haiku-5-5"), Some(ModelTier::Lightweight));
        assert!(model_tier("sonnet") < model_tier("opus"));
        assert!(model_tier("opus") < model_tier("claude-fable-5-2"));
        assert_eq!(model_tier("gpt-5"), None);
    }
}
