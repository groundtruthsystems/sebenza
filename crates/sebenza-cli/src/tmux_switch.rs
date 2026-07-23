//! Switch the terminal to a worktree's tmux window after `add`/`open`/`restore`.

use std::process::{Command, Stdio};

use common::adapters::tmux::{build_project_session_name, build_worktree_window_name};

pub fn switch_to_window(project_dir: &str, branch: &str) {
    let session = build_project_session_name(project_dir);
    let window = build_worktree_window_name(branch);
    let target = format!("{session}:{window}");

    let selected = Command::new("tmux")
        .args(["select-window", "-t", &target])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !selected {
        return;
    }

    if std::env::var_os("TMUX").is_some() {
        let ok = Command::new("tmux")
            .args(["switch-client", "-t", &session])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("Warning: failed to switch tmux client to {session}");
        }
    } else {
        let ok = Command::new("tmux")
            .args(["attach-session", "-t", &session])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("Warning: failed to attach to tmux session {session}");
        }
    }
}
