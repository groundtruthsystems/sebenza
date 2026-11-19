//! `sebenza-cli track complete <track-id>`: complete a track through the
//! daemon, as `sebenza-agentctl complete` does. The daemon records the
//! completion and its review job and marks the track done in `tracks.json`.

use anyhow::{Result, anyhow};

use crate::http::Http;

#[derive(Debug, PartialEq, Eq)]
enum TrackCommand {
    Complete {
        track_id: String,
        /// Defaults to the id in this worktree's `control.env`.
        worktree_id: Option<String>,
    },
}

fn usage() -> String {
    [
        "Usage:",
        "  sebenza-cli track complete <track-id> [--worktree-id <id>]",
        "",
        "Completes a track of the current worktree through the daemon: it records the",
        "completion, enqueues the track's standards review and marks it done in",
        "tracks.json. The worktree id defaults to the one Sebenza wrote for this",
        "worktree (<git-dir>/.ai/sebenza/control.env).",
    ]
    .join("\n")
}

fn parse(args: &[String]) -> Result<Option<TrackCommand>> {
    let Some(sub) = args.first().map(String::as_str) else {
        return Ok(None);
    };
    match sub {
        "--help" | "-h" => Ok(None),
        "complete" => parse_complete(&args[1..]).map(Some),
        other => Err(anyhow!("Unknown track subcommand: {other}")),
    }
}

fn parse_complete(args: &[String]) -> Result<TrackCommand> {
    let mut track_id = None;
    let mut worktree_id = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--worktree-id" => {
                let v = it
                    .next()
                    .ok_or_else(|| anyhow!("--worktree-id needs a value"))?;
                worktree_id = Some(v.clone());
            }
            flag if flag.starts_with('-') => return Err(anyhow!("Unknown option: {flag}")),
            id if track_id.is_none() => track_id = Some(id.to_string()),
            extra => return Err(anyhow!("Unexpected argument: {extra}")),
        }
    }
    let track_id = track_id.ok_or_else(|| anyhow!("track complete requires a <track-id>"))?;
    Ok(TrackCommand::Complete {
        track_id,
        worktree_id,
    })
}

/// The worktree id Sebenza wrote into `<git-dir>/.ai/sebenza/control.env`.
fn worktree_id_from(cwd: &str) -> Result<String> {
    let git_dir = common::adapters::git::GitGateway::new()
        .resolve_worktree_git_dir(cwd)
        .map_err(|e| anyhow!("not inside a git worktree: {e}"))?;
    let paths = common::adapters::fs::get_worktree_storage_paths(&git_dir);
    let text = std::fs::read_to_string(&paths.control_env_path)
        .map_err(|_| anyhow!("this worktree is not managed by Sebenza (no control.env)"))?;
    worktree_id_in(&text)
}

fn worktree_id_in(control_env: &str) -> Result<String> {
    common::adapters::fs::parse_dotenv(control_env)
        .remove("SEBENZA_WORKTREE_ID")
        .filter(|id| !id.is_empty())
        .ok_or_else(|| anyhow!("control.env names no SEBENZA_WORKTREE_ID"))
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
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

async fn run_inner(cmd: TrackCommand, port: u16, cwd: &str) -> Result<()> {
    let TrackCommand::Complete {
        track_id,
        worktree_id,
    } = cmd;
    let worktree_id = match worktree_id {
        Some(id) => id,
        None => worktree_id_from(cwd)?,
    };
    let done = Http::new(port)
        .complete_track(&track_id, &worktree_id)
        .await?;
    let job = &done["job"];
    let verb = if done["appended"] == true {
        "completed"
    } else {
        "was already completed"
    };
    println!(
        "Track {track_id} {verb} at {} (base {}); review job queued.",
        job["headSha"].as_str().unwrap_or("?"),
        job["baseSha"].as_str().unwrap_or("?"),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn complete_takes_a_track_and_an_optional_worktree_id() {
        assert_eq!(
            parse(&a(&["complete", "t_20261118"])).unwrap(),
            Some(TrackCommand::Complete {
                track_id: "t_20261118".into(),
                worktree_id: None
            })
        );
        assert_eq!(
            parse(&a(&["complete", "--worktree-id", "wt-1", "t_20261118"])).unwrap(),
            Some(TrackCommand::Complete {
                track_id: "t_20261118".into(),
                worktree_id: Some("wt-1".into())
            })
        );
    }

    #[test]
    fn complete_without_a_track_or_with_extras_is_an_error() {
        assert!(parse(&a(&["complete"])).is_err());
        assert!(parse(&a(&["complete", "a", "b"])).is_err());
        assert!(parse(&a(&["complete", "a", "--force"])).is_err());
        assert!(parse(&a(&["complete", "a", "--worktree-id"])).is_err());
        assert!(parse(&a(&["finish", "a"])).is_err());
    }

    #[test]
    fn help_and_empty_render_usage() {
        assert_eq!(parse(&[]).unwrap(), None);
        assert_eq!(parse(&a(&["--help"])).unwrap(), None);
    }

    #[test]
    fn the_worktree_id_comes_from_control_env() {
        let env = "SEBENZA_CONTROL_URL=http://127.0.0.1:5111/api/runtime/events\n\
                   SEBENZA_WORKTREE_ID=wt-id-1\nSEBENZA_BRANCH=feat\n";
        assert_eq!(worktree_id_in(env).unwrap(), "wt-id-1");
        assert!(worktree_id_in("SEBENZA_BRANCH=feat\n").is_err());
    }
}
