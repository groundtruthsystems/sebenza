//! CLI parity for the software factory (application-architecture.md,
//! Interfaces): `sebenza-cli inbox solution|coordinator|factory …`.
//!
//! Each command maps to one route. Operator-only routes use the token from
//! `sebenza-cli login`; the rest use the control token.

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub method: &'static str,
    pub path: String,
    pub body: Option<Value>,
    /// Needs the operator token (`sebenza-cli login`).
    pub operator: bool,
}

fn get(path: String) -> Call {
    Call {
        method: "GET",
        path,
        body: None,
        operator: false,
    }
}

fn op(method: &'static str, path: String, body: Value) -> Call {
    Call {
        method,
        path,
        body: Some(body),
        operator: true,
    }
}

/// `agent[:model]` -> `{agent, model?}`.
pub fn profile(spec: &str) -> Value {
    match spec.split_once(':') {
        Some((a, m)) if !m.is_empty() => json!({"agent": a, "model": m}),
        _ => json!({"agent": spec.trim_end_matches(':')}),
    }
}

/// Set one role in a `factory:`-shaped value (global) or overrides (item).
fn set_role(target: &mut Value, role: &str, spec: &str, global: bool) -> Result<()> {
    let p = profile(spec);
    let slot = match (role, global) {
        ("coordinator", true) => &mut target["solution"]["coordinator"],
        ("designer", true) => &mut target["solution"]["designer"],
        ("solution-reviewer", true) => &mut target["solution"]["reviewer"],
        ("project-reviewer", true) => &mut target["project"]["reviewer"],
        ("coordinator", false) => &mut target["coordinator"],
        ("designer", false) => &mut target["designer"],
        ("solution-reviewer", false) => &mut target["reviewer"]["solution"],
        ("project-reviewer", false) => &mut target["reviewer"]["project"],
        _ => {
            return Err(anyhow!(
                "role must be coordinator, designer, solution-reviewer or project-reviewer"
            ));
        }
    };
    *slot = p;
    Ok(())
}

pub fn usage() -> &'static str {
    "  sebenza-cli inbox new <title> --kind solution    Create a solution item
  sebenza-cli inbox solution show <id>             Phase, round n of 3, problems, documents
  sebenza-cli inbox solution start <id>            Start design (operator)
  sebenza-cli inbox solution review <id> [--new-cycle]   Next review round (operator)
  sebenza-cli inbox solution doc <id> <name>       Print a solution document
  sebenza-cli inbox solution resolve <id> <problem> [note]   Resolve a problem by hand (operator)
  sebenza-cli inbox coordinator say <id> <message> [--confirm]   Chat with the coordinator (operator)
  sebenza-cli inbox coordinator log <id>           Print the coordinator transcript
  sebenza-cli inbox factory get                    Global role profiles
  sebenza-cli inbox factory agents                 Agents for role jobs (installed, verified)
  sebenza-cli inbox factory set <role> <agent[:model]>   Set a global role (operator)
  sebenza-cli inbox factory item <id> <role> <agent[:model]>   Set an item's role (operator)
    roles: coordinator, designer, solution-reviewer, project-reviewer"
}

/// Map `args` (starting at `solution`, `coordinator` or `factory`) to the
/// route call(s). `current` supplies the existing config or overrides for a
/// `set`, which rewrites one role and keeps the rest.
pub fn parse(args: &[String], current: Option<&Value>) -> Result<Call> {
    let a = |i: usize, what: &str| -> Result<String> {
        args.iter()
            .filter(|s| !s.starts_with("--"))
            .nth(i)
            .cloned()
            .ok_or_else(|| anyhow!("Missing {what}"))
    };
    let has = |f: &str| args.iter().any(|s| s == f);
    match (a(0, "group")?.as_str(), a(1, "command")?.as_str()) {
        ("solution", "show") => Ok(get(format!("/api/inbox/{}/solution", a(2, "item id")?))),
        ("solution", "start") => Ok(op(
            "POST",
            format!("/api/inbox/{}/solution/start-design", a(2, "item id")?),
            json!({}),
        )),
        ("solution", "review") => Ok(op(
            "POST",
            format!("/api/inbox/{}/solution/review", a(2, "item id")?),
            json!({"newCycle": has("--new-cycle")}),
        )),
        ("solution", "doc") => Ok(get(format!(
            "/api/inbox/{}/solution/docs/{}",
            a(2, "item id")?,
            a(3, "document name")?
        ))),
        ("solution", "resolve") => Ok(op(
            "POST",
            format!(
                "/api/inbox/{}/solution/problems/{}/resolve",
                a(2, "item id")?,
                a(3, "problem id")?
            ),
            json!({"note": a(4, "note").unwrap_or_default()}),
        )),
        ("coordinator", "say") => Ok(op(
            "POST",
            format!("/api/inbox/{}/coordinator/turn", a(2, "item id")?),
            json!({"message": a(3, "message")?, "confirmWarnings": has("--confirm")}),
        )),
        ("coordinator", "log") => Ok(get(format!(
            "/api/inbox/{}/coordinator/transcript",
            a(2, "item id")?
        ))),
        ("factory", "get") => Ok(get("/api/factory/config".into())),
        ("factory", "agents") => Ok(get("/api/factory/agents".into())),
        ("factory", "set") => {
            let mut cfg = current
                .and_then(|c| c.get("config"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            set_role(&mut cfg, &a(2, "role")?, &a(3, "agent[:model]")?, true)?;
            Ok(op("PUT", "/api/factory/config".into(), cfg))
        }
        ("factory", "item") => {
            let id = a(2, "item id")?;
            let mut o = current
                .and_then(|c| c.get("overrides"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            set_role(&mut o, &a(3, "role")?, &a(4, "agent[:model]")?, false)?;
            Ok(op("PUT", format!("/api/inbox/{id}/factory/agents"), o))
        }
        (g, c) => Err(anyhow!("unknown command: inbox {g} {c}")),
    }
}

/// The GET whose answer a `set` builds on.
pub fn current_for(args: &[String]) -> Option<String> {
    let pos: Vec<&String> = args.iter().filter(|s| !s.starts_with("--")).collect();
    match (pos.first()?.as_str(), pos.get(1)?.as_str()) {
        ("factory", "set") => Some("/api/factory/config".into()),
        ("factory", "item") => pos
            .get(2)
            .map(|id| format!("/api/inbox/{id}/factory/agents")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    // FR-24: reviewer settings through the CLI, global and per item, keeping
    // the other roles.
    #[test]
    fn factory_set_rewrites_one_role() {
        let current =
            json!({"config": {"solution": {"designer": {"agent": "claude", "model": "opus"}}}});
        let c = parse(
            &v(&[
                "factory",
                "set",
                "solution-reviewer",
                "claude:claude-fable-5-2",
            ]),
            Some(&current),
        )
        .unwrap();
        assert_eq!(
            (c.method, c.path.as_str(), c.operator),
            ("PUT", "/api/factory/config", true)
        );
        let body = c.body.unwrap();
        assert_eq!(body["solution"]["designer"]["model"], "opus");
        assert_eq!(
            body["solution"]["reviewer"],
            json!({"agent": "claude", "model": "claude-fable-5-2"})
        );

        let c = parse(
            &v(&["factory", "item", "01ID", "project-reviewer", "codex"]),
            Some(&json!({"overrides": {}})),
        )
        .unwrap();
        assert_eq!(c.path, "/api/inbox/01ID/factory/agents");
        assert_eq!(
            c.body.unwrap(),
            json!({"reviewer": {"project": {"agent": "codex"}}})
        );
        assert!(parse(&v(&["factory", "set", "boss", "codex"]), None).is_err());
        assert_eq!(
            current_for(&v(&["factory", "item", "01ID", "designer", "codex"])).as_deref(),
            Some("/api/inbox/01ID/factory/agents")
        );
    }

    #[test]
    fn solution_and_coordinator_commands_map_to_routes() {
        let c = parse(&v(&["solution", "review", "01ID", "--new-cycle"]), None).unwrap();
        assert_eq!(c.path, "/api/inbox/01ID/solution/review");
        assert_eq!(c.body.unwrap()["newCycle"], true);
        assert!(
            parse(&v(&["solution", "start", "01ID"]), None)
                .unwrap()
                .operator
        );
        assert!(
            !parse(&v(&["solution", "show", "01ID"]), None)
                .unwrap()
                .operator
        );
        let c = parse(
            &v(&["coordinator", "say", "01ID", "hello there", "--confirm"]),
            None,
        )
        .unwrap();
        assert_eq!(
            c.body.unwrap(),
            json!({"message": "hello there", "confirmWarnings": true})
        );
        assert_eq!(
            parse(&v(&["coordinator", "log", "01ID"]), None)
                .unwrap()
                .method,
            "GET"
        );
        assert_eq!(
            parse(&v(&["solution", "doc", "01ID", "handover/acme.md"]), None)
                .unwrap()
                .path,
            "/api/inbox/01ID/solution/docs/handover/acme.md"
        );
        assert!(parse(&v(&["solution", "explode", "01ID"]), None).is_err());
    }
}
