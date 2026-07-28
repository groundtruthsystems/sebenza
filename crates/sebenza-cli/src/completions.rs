//! `sebenza-cli completion <bash|zsh>` prints a shell completion script; the internal
//! `sebenza-cli --completions <sub>` (invoked by those scripts) lists worktree branches
//! for dynamic completion. Branch listing shells out to git — no server needed.

use std::path::Path;

use common::adapters::git::{canonical_path, split_repo_root_entry, GitGateway};
use common::config::project_root;

/// Subcommands that take a `<branch>` argument (get dynamic completion).
const BRANCH_SUBCOMMANDS: &[&str] = &[
    "open", "close", "refresh", "archive", "unarchive", "label", "remove", "merge", "send",
];

const USAGE: &str = "Usage:
  sebenza-cli completion <bash|zsh>

Add this to your shell config to enable autocompletion:

  # ~/.zshrc
  eval \"$(sebenza-cli completion zsh)\"

  # ~/.bashrc
  eval \"$(sebenza-cli completion bash)\"";

const ZSH_SCRIPT: &str = r#"#compdef sebenza-cli
_sebenza_cli() {
  local -a commands
  commands=(
    'serve:Start the dashboard server'
    'init:Interactive project setup'
    'service:Manage sebenza-cli as a system service'
    'update:Update sebenza-cli to the latest version'
    'add:Create a worktree'
    'oneshot:Run a worktree start-to-finish'
    'list:List worktrees'
    'open:Open a worktree session'
    'close:Close a worktree session'
    'refresh:Refresh an agent terminal'
    'archive:Archive a worktree'
    'unarchive:Unarchive a worktree'
    'label:Set or clear a workspace label'
    'remove:Remove a worktree'
    'merge:Merge a worktree into the main branch'
    'send:Send a prompt to a worktree agent'
    'prune:Remove all closed worktrees'
    'restore:Re-open previously open sessions'
    'project:Manage served projects'
    'completion:Generate a shell completion script'
  )
  if (( CURRENT == 2 )); then
    _describe 'command' commands
    return
  fi
  case "${words[2]}" in
    open|close|refresh|archive|unarchive|label|remove|merge|send)
      if (( CURRENT == 3 )); then
        local -a branches
        branches=(${(f)"$(sebenza-cli --completions "${words[2]}" 2>/dev/null)"})
        (( ${#branches} )) && _describe 'worktree' branches
      fi
      ;;
    project)
      if (( CURRENT == 3 )); then
        _describe 'subcommand' '(ls add rm migrate)'
      fi
      ;;
    completion)
      if (( CURRENT == 3 )); then
        _describe 'shell' '(bash zsh)'
      fi
      ;;
    service)
      if (( CURRENT == 3 )); then
        _describe 'action' '(install uninstall status logs)'
      fi
      ;;
  esac
}
compdef _sebenza_cli sebenza-cli
"#;

const BASH_SCRIPT: &str = r#"_sebenza_cli() {
  local cur prev
  cur="${COMP_WORDS[COMP_CWORD]}"
  if (( COMP_CWORD == 1 )); then
    COMPREPLY=( $(compgen -W "serve init service update add oneshot list open close refresh archive unarchive label remove merge send prune restore project completion" -- "$cur") )
    return
  fi
  case "${COMP_WORDS[1]}" in
    open|close|refresh|archive|unarchive|label|remove|merge|send)
      if (( COMP_CWORD == 2 )); then
        local branches
        branches=$(sebenza-cli --completions "${COMP_WORDS[1]}" 2>/dev/null)
        COMPREPLY=( $(compgen -W "${branches}" -- "$cur") )
      fi
      ;;
    project)
      (( COMP_CWORD == 2 )) && COMPREPLY=( $(compgen -W "ls add rm migrate" -- "$cur") )
      ;;
    completion)
      (( COMP_CWORD == 2 )) && COMPREPLY=( $(compgen -W "bash zsh" -- "$cur") )
      ;;
    service)
      (( COMP_CWORD == 2 )) && COMPREPLY=( $(compgen -W "install uninstall status logs" -- "$cur") )
      ;;
  esac
}
complete -F _sebenza_cli sebenza-cli
"#;

/// `sebenza-cli completion <shell>` — print the completion script.
pub fn run_completion_command(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        None | Some("--help") | Some("-h") => {
            println!("{USAGE}");
            0
        }
        Some("bash") => {
            println!("{BASH_SCRIPT}");
            0
        }
        Some("zsh") => {
            println!("{ZSH_SCRIPT}");
            0
        }
        Some(other) => {
            eprintln!("Unknown shell: {other}. Supported: bash, zsh");
            1
        }
    }
}

/// Internal `sebenza-cli --completions <sub>` — print one branch per line for the
/// dynamic completion of branch-taking subcommands.
pub fn handle_completions(args: &[String]) {
    let Some(sub) = args.first() else { return };
    if !BRANCH_SUBCOMMANDS.contains(&sub.as_str()) {
        return;
    }
    for branch in list_worktree_branches() {
        println!("{branch}");
    }
}

fn list_worktree_branches() -> Vec<String> {
    let cwd = std::env::current_dir()
        .map(|c| c.to_string_lossy().to_string())
        .unwrap_or_default();
    let root = project_root(&cwd);
    let root_canon = canonical_path(&root);
    // The repo root is included: `sebenza-cli open main` / `close main` are valid
    // now that the main checkout can be opened as a terminal session.
    let (root_entry, linked) =
        split_repo_root_entry(GitGateway::new().list_worktrees(&cwd), &root_canon);
    root_entry
        .into_iter()
        .chain(linked)
        .map(|e| {
            e.branch.clone().unwrap_or_else(|| {
                Path::new(&e.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            })
        })
        .collect()
}
