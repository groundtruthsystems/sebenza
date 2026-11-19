//! `sebenza-cli review status|findings <track-id>`: read a track's
//! standards review from the daemon. Read-only: nothing here moves a job, a
//! finding or a verdict.

use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::http::Http;

#[derive(Debug, PartialEq, Eq)]
enum ReviewCommand {
    Status { track_id: String },
    Findings { track_id: String },
}

fn usage() -> String {
    [
        "Usage:",
        "  sebenza-cli review status <track-id>",
        "  sebenza-cli review findings <track-id>",
        "",
        "Shows the standards review of a track in the current project: the latest",
        "job's state, attempts and daemon-derived verdict, or every finding with its",
        "status. Read-only.",
    ]
    .join("\n")
}

fn parse(args: &[String]) -> Result<Option<ReviewCommand>> {
    let Some(sub) = args.first().map(String::as_str) else {
        return Ok(None);
    };
    if matches!(sub, "--help" | "-h") {
        return Ok(None);
    }
    let track_id = match &args[1..] {
        [] => return Err(anyhow!("review {sub} requires a <track-id>")),
        [first, ..] if first.starts_with('-') => return Err(anyhow!("Unknown option: {first}")),
        [id] => id.clone(),
        [_, extra, ..] => return Err(anyhow!("Unexpected argument: {extra}")),
    };
    match sub {
        "status" => Ok(Some(ReviewCommand::Status { track_id })),
        "findings" => Ok(Some(ReviewCommand::Findings { track_id })),
        other => Err(anyhow!("Unknown review subcommand: {other}")),
    }
}

pub async fn run(args: &[String], port: u16, cwd: &str) -> i32 {
    let cmd = match parse(args) {
        Ok(Some(cmd)) => cmd,
        Ok(None) => {
            println!("{}", usage());
            return 0;
        }
        Err(e) => {
            eprintln!("{e}\n{}", usage());
            return 1;
        }
    };
    match run_inner(cmd, port, cwd).await {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

async fn run_inner(cmd: ReviewCommand, port: u16, cwd: &str) -> Result<String> {
    let http = Http::new(port);
    let base = http.resolve_project_base(cwd).await?;
    match cmd {
        ReviewCommand::Status { track_id } => {
            Ok(render_status(&http.review_status(&base, &track_id).await?))
        }
        ReviewCommand::Findings { track_id } => Ok(render_findings(
            &http.review_findings(&base, &track_id).await?,
        )),
    }
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("?")
}

fn render_status(v: &Value) -> String {
    let track = s(&v["trackId"]);
    let job = &v["job"];
    if job.is_null() {
        return format!("Track {track} has no review job.");
    }
    let mut lines = vec![
        format!("Track {track}: {}", s(&job["state"])),
        format!("  job       {}", s(&job["jobId"])),
        format!("  head      {}", s(&job["headSha"])),
        format!("  base      {}", s(&job["baseSha"])),
        format!(
            "  attempts  first {} / second {}",
            job["attempts"]["first"], job["attempts"]["second"]
        ),
    ];
    for b in job["blockers"].as_array().into_iter().flatten() {
        lines.push(format!(
            "  blocked   {} {}",
            s(&b["reason"]),
            b["path"].as_str().unwrap_or("")
        ));
    }
    lines.push(match job["verdict"]["passing"].as_bool() {
        None => "  verdict   none yet".to_string(),
        Some(true) => "  verdict   passing".to_string(),
        Some(false) => format!(
            "  verdict   not passing ({})",
            reasons(&job["verdict"]["reasons"])
        ),
    });
    lines.join("\n")
}

fn reasons(v: &Value) -> String {
    v.as_array()
        .map(|r| r.iter().map(s).collect::<Vec<_>>().join(", "))
        .unwrap_or_default()
}

fn render_findings(v: &Value) -> String {
    let findings = v["findings"].as_array().cloned().unwrap_or_default();
    if findings.is_empty() {
        return format!("Track {} has no findings.", s(&v["trackId"]));
    }
    findings
        .iter()
        .map(|f| {
            format!(
                "{}  {:<24} {}:{}  [{}]  {}",
                s(&f["findingId"]),
                s(&f["status"]),
                s(&f["anchor"]["path"]),
                f["anchor"]["line"],
                s(&f["ruleId"]),
                s(&f["description"]),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn status_and_findings_take_one_track_id() {
        assert_eq!(
            parse(&a(&["status", "t_1"])).unwrap(),
            Some(ReviewCommand::Status {
                track_id: "t_1".into()
            })
        );
        assert_eq!(
            parse(&a(&["findings", "t_1"])).unwrap(),
            Some(ReviewCommand::Findings {
                track_id: "t_1".into()
            })
        );
        assert_eq!(parse(&a(&[])).unwrap(), None);
        assert_eq!(parse(&a(&["--help"])).unwrap(), None);
        assert!(parse(&a(&["status"])).is_err());
        assert!(parse(&a(&["status", "a", "b"])).is_err());
        assert!(parse(&a(&["status", "--json"])).is_err());
        assert!(
            parse(&a(&["approve", "t_1"])).is_err(),
            "read-only: no mutating subcommand"
        );
    }

    #[test]
    fn status_renders_state_attempts_and_verdict() {
        let v = json!({"trackId": "t_1", "job": {
            "jobId": "job_1", "headSha": "h", "baseSha": "b", "state": "verdict",
            "attempts": {"first": 1, "second": 0}, "blockers": [],
            "verdict": {"passing": false, "reasons": ["open-findings", "existing-checks-missing"]}}});
        let text = render_status(&v);
        assert!(text.contains("Track t_1: verdict"), "{text}");
        assert!(text.contains("first 1 / second 0"), "{text}");
        assert!(
            text.contains("not passing (open-findings, existing-checks-missing)"),
            "{text}"
        );
        assert_eq!(
            render_status(&json!({"trackId": "t_1", "job": null})),
            "Track t_1 has no review job."
        );
    }

    #[test]
    fn findings_render_one_line_each() {
        let v = json!({"trackId": "t_1", "findings": [{
            "findingId": "f_1", "status": "open", "ruleId": "short",
            "anchor": {"path": "src/a.rs", "line": 3}, "description": "Too long."}]});
        let text = render_findings(&v);
        assert!(text.starts_with("f_1  open"), "{text}");
        assert!(text.contains("src/a.rs:3  [short]  Too long."), "{text}");
        assert_eq!(
            render_findings(&json!({"trackId": "t_1", "findings": []})),
            "Track t_1 has no findings."
        );
    }
}
