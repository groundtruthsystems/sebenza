use crate::util::shell::{detect_project_name, run, which};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const ANALYZE_TIMEOUT: Duration = Duration::from_secs(120);

const FAST_CLAUDE_MODEL: &str = "haiku";
const FAST_CLAUDE_EFFORT: &str = "low";
const FAST_CODEX_MODEL: &str = "gpt-5.1-codex";
const FAST_CODEX_REASONING: &str = "low";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InitAgent {
    Claude,
    Codex,
}

impl InitAgent {
    fn as_str(self) -> &'static str {
        match self {
            InitAgent::Claude => "claude",
            InitAgent::Codex => "codex",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PackageManager {
    Bun,
    Npm,
    Pnpm,
    Yarn,
}

pub struct InitProjectContext {
    pub git_root: String,
    pub project_name: String,
    pub main_branch: String,
    pub default_agent: InitAgent,
    package_manager: PackageManager,
}

/// The agent used for authoring: codex only if it's the sole one available.
pub fn authoring_agent() -> InitAgent {
    if which("codex") && !which("claude") {
        InitAgent::Codex
    } else {
        InitAgent::Claude
    }
}

fn detect_package_manager(git_root: &str) -> PackageManager {
    let has = |name: &str| Path::new(git_root).join(name).exists();
    if has("bun.lock") || has("bun.lockb") {
        PackageManager::Bun
    } else if has("pnpm-lock.yaml") {
        PackageManager::Pnpm
    } else if has("yarn.lock") {
        PackageManager::Yarn
    } else {
        PackageManager::Npm
    }
}

fn detect_main_branch(git_root: &str) -> String {
    let cwd = Some(Path::new(git_root));
    let trimmed = |r: crate::util::shell::RunResult| -> Option<String> {
        r.success
            .then(|| String::from_utf8_lossy(&r.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    };

    if let Some(head) = trimmed(run(
        "git",
        &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"],
        cwd,
    )) && let Some(branch) = head.rsplit('/').next()
        && !branch.is_empty()
    {
        return branch.to_string();
    }
    if trimmed(run("git", &["branch", "--list", "main"], cwd)).is_some() {
        return "main".to_string();
    }
    if trimmed(run("git", &["branch", "--list", "master"], cwd)).is_some() {
        return "master".to_string();
    }
    if let Some(branch) = trimmed(run("git", &["rev-parse", "--abbrev-ref", "HEAD"], cwd))
        && branch != "HEAD"
    {
        return branch;
    }
    "main".to_string()
}

fn run_script_command(pm: PackageManager, script: &str) -> String {
    match pm {
        PackageManager::Bun => format!("bun run {script}"),
        PackageManager::Pnpm => format!("pnpm {script}"),
        PackageManager::Yarn => format!("yarn {script}"),
        PackageManager::Npm => format!("npm run {script}"),
    }
}

pub fn detect_init_project_context(git_root: &str, default_agent: InitAgent) -> InitProjectContext {
    InitProjectContext {
        git_root: git_root.to_string(),
        project_name: detect_project_name(git_root),
        main_branch: detect_main_branch(git_root),
        default_agent,
        package_manager: detect_package_manager(git_root),
    }
}

pub struct InitPromptSpec {
    pub system_prompt: String,
    pub user_prompt: String,
}

pub fn build_init_prompt_spec(context: &InitProjectContext) -> InitPromptSpec {
    let system_prompt = [
        "You are bootstrapping a local repository for Sebenza.".to_string(),
        "A starter `.ai/sebenza.yaml` already exists at the repo root.".to_string(),
        "Inspect the repository in the current working directory and edit that existing `.ai/sebenza.yaml` in place.".to_string(),
        "Do not modify any other file.".to_string(),
        "Do not ask the user questions. Infer the config from the repository contents.".to_string(),
        "Be efficient: inspect only the files needed to determine the project name, main branch, service layout, dev commands, and ports.".to_string(),
        "The active, uncommented YAML must be valid and minimal.".to_string(),
        "Do not remove other starter sections or their explanatory comments just because they are unused.".to_string(),
        "Keep optional examples and comments in place so the user can uncomment and use them later.".to_string(),
        format!("Set workspace.defaultAgent to {}.", context.default_agent.as_str()),
        "Use this config shape:".to_string(),
        "name: infer from the repository".to_string(),
        "workspace.mainBranch: infer from git".to_string(),
        "workspace.worktreeRoot: keep ../worktrees unless there is clear evidence of an existing alternative".to_string(),
        "services: one entry per real dev service with name, portEnv, and portStart when a default port is clear".to_string(),
        "profiles.default.runtime: host".to_string(),
        "profiles.default.envPassthrough: []".to_string(),
        "profiles.default.panes: start with an agent pane focused true, then add command panes for real services".to_string(),
        "Command panes should use the repository's real dev command and pass the relevant port env var into the command when needed.".to_string(),
        "Use split: right for the first command pane and split: bottom for later command panes.".to_string(),
        "Include integrations.github.linkedRepos as an empty list and startupEnvs as an empty object.".to_string(),
        "Only include optional sections like auto_name, lifecycleHooks, sandbox/docker config, mounts, or systemPrompt if the repository gives clear evidence they are needed.".to_string(),
        "Prefer editing the existing keys over replacing the file with a completely different shape.".to_string(),
        "Preserve the existing template structure and comments unless a specific change requires updating them.".to_string(),
        "Before finishing, verify that `.ai/sebenza.yaml` exists and contains the final YAML.".to_string(),
    ]
    .join("\n");

    InitPromptSpec {
        system_prompt,
        user_prompt: "Adapt the existing starter `.ai/sebenza.yaml` for this repository.".to_string(),
    }
}

pub struct InitAgentCommandSpec {
    pub cmd: String,
    pub args: Vec<String>,
    pub summary_path: Option<String>,
}

/// Build the headless-agent invocation. `unique` disambiguates the codex summary
/// file; the caller passes a stamp.
pub fn build_init_agent_command(
    agent: InitAgent,
    prompt: &InitPromptSpec,
    unique: &str,
) -> InitAgentCommandSpec {
    let s = |v: &str| v.to_string();
    if agent == InitAgent::Claude {
        return InitAgentCommandSpec {
            cmd: s("claude"),
            args: vec![
                s("-p"),
                s("--verbose"),
                s("--permission-mode"),
                s("bypassPermissions"),
                s("--model"),
                s(FAST_CLAUDE_MODEL),
                s("--effort"),
                s(FAST_CLAUDE_EFFORT),
                s("--output-format"),
                s("stream-json"),
                s("--include-partial-messages"),
                s("--append-system-prompt"),
                prompt.system_prompt.clone(),
                prompt.user_prompt.clone(),
            ],
            summary_path: None,
        };
    }

    let summary_path = std::env::temp_dir()
        .join(format!("sebenza-init-{unique}.txt"))
        .to_string_lossy()
        .to_string();
    InitAgentCommandSpec {
        cmd: s("codex"),
        args: vec![
            s("exec"),
            s("--sandbox"),
            s("workspace-write"),
            s("--color"),
            s("never"),
            s("--json"),
            s("-m"),
            s(FAST_CODEX_MODEL),
            s("-o"),
            summary_path.clone(),
            s("-c"),
            format!("model_reasoning_effort=\"{FAST_CODEX_REASONING}\""),
            s("-c"),
            format!("developer_instructions={}", prompt.system_prompt),
            prompt.user_prompt.clone(),
        ],
        summary_path: Some(summary_path),
    }
}

/// Run the authoring agent to completion (or until `timeout`), letting it edit
/// `.ai/sebenza.yaml` in `cwd`. Output is discarded (the server surfaces only the
/// phase); the codex summary temp file is cleaned up. Errors are returned so the
/// caller can treat analysis as best-effort.
pub fn run_init_agent_command(
    spec: &InitAgentCommandSpec,
    cwd: &str,
    timeout: Duration,
) -> Result<(), String> {
    let mut child = Command::new(&spec.cmd)
        .args(&spec.args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", spec.cmd))?;

    let deadline = Instant::now() + timeout;
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(status)) => break Err(format!("{} exited with {status}", spec.cmd)),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    break Err(format!("{} timed out", spec.cmd));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => break Err(e.to_string()),
        }
    };

    if let Some(path) = &spec.summary_path {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// Write a starter `.ai/sebenza.yaml` to the repo root, then best-effort run
/// the authoring agent to flesh it out (agent step skipped if no CLI is available).
pub fn scaffold_config(context: &InitProjectContext) -> Result<(), String> {
    let template = build_starter_template(context);
    let dir = Path::new(&context.git_root).join(".ai");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("sebenza.yaml"), template).map_err(|e| e.to_string())
}

pub fn analyze_config(context: &InitProjectContext, unique: &str) -> Result<(), String> {
    let prompt = build_init_prompt_spec(context);
    let spec = build_init_agent_command(context.default_agent, &prompt, unique);
    run_init_agent_command(&spec, &context.git_root, ANALYZE_TIMEOUT)
}

pub fn build_starter_template(context: &InitProjectContext) -> String {
    let dev_command = run_script_command(context.package_manager, "dev");
    let default_agent = context.default_agent.as_str();
    let auto_name_model = if context.default_agent == InitAgent::Codex {
        "gpt-5.1-codex"
    } else {
        "claude-3-5-haiku-latest"
    };
    // Literal `${PORT}` / `${SEBENZA_WORKTREE_PATH}` in the template must survive
    // verbatim, so substitute via named tokens rather than shell/format braces.
    STARTER_TEMPLATE
        .replace("__PROJECT_NAME__", &context.project_name)
        .replace("__MAIN_BRANCH__", &context.main_branch)
        .replace("__DEFAULT_AGENT__", default_agent)
        .replace("__DEV_COMMAND__", &dev_command)
        .replace("__AUTONAME_MODEL__", auto_name_model)
}

const STARTER_TEMPLATE: &str = r#"# Starter config for Sebenza.
# Keep the active keys below as a minimal working setup, then uncomment
# the examples to enable more services, profiles, integrations, or hooks.

# Project display name shown in the dashboard and browser title.
name: __PROJECT_NAME__

workspace:
  # Git branch new worktrees start from.
  mainBranch: __MAIN_BRANCH__
  # Relative or absolute directory where managed worktrees are created.
  worktreeRoot: ../worktrees
  # Agent new worktrees use by default.
  defaultAgent: __DEFAULT_AGENT__
  # Example background pull settings for keeping the main branch fresh.
  # autoPull:
  #   # Turn automatic pulls on or off.
  #   enabled: false
  #   # Seconds between pull attempts.
  #   intervalSeconds: 300

# Services define the ports Sebenza allocates and tracks per worktree.
services:
  # Example app service with a predictable per-worktree port.
  # - name: app
  #   # Env var name injected into panes and hooks.
  #   portEnv: PORT
  #   # Starting port for the first worktree slot.
  #   portStart: 3000
  #   # Port increment between worktree slots.
  #   portStep: 10
  #   # Link shown in the dashboard when the service is running.
  #   urlTemplate: http://localhost:${PORT}

# Profiles define runtime, permissions, and tmux pane layout.
profiles:
  default:
    # Run panes directly on the host machine.
    runtime: host
    # Forward selected host env vars into the agent process.
    envPassthrough:
      # - ANTHROPIC_API_KEY
      # - OPENAI_API_KEY
    # Extra system instructions for the agent in this profile.
    # systemPrompt: >
    #   You are working in ${SEBENZA_WORKTREE_PATH}
    # Skip agent permission prompts in this profile.
    # yolo: true
    # Panes define the tmux layout created for each worktree session.
    panes:
      # Main AI coding pane.
      - id: agent
        # Pane type: agent, command, or shell.
        kind: agent
        # Focus this pane when the session opens.
        focus: true
        # Place this pane to the right of the existing layout.
        # split: right
        # Percent of the available space this pane should take.
        # sizePct: 50
        # Start this pane in the repo root or managed worktree.
        # cwd: worktree
      # Example dev server pane.
      # - id: app
      #   # Pane type: agent, command, or shell.
      #   kind: command
      #   # Place this pane to the right of the existing layout.
      #   split: right
      #   # Percent of the available space this pane should take.
      #   sizePct: 50
      #   # Start this pane in the repo root or managed worktree.
      #   cwd: worktree
      #   # Change into a subdirectory before running the command.
      #   workingDir: frontend
      #   # Command run when the pane starts. Sebenza injects $PORT.
      #   command: PORT=$PORT __DEV_COMMAND__
      # Example shell pane for manual commands.
      # - id: shell
      #   # Pane type: agent, command, or shell.
      #   kind: shell
      #   # Place this pane below the existing layout.
      #   split: bottom
      #   # Percent of the available space this pane should take.
      #   sizePct: 30
      #   # Start this pane in the repo root or managed worktree.
      #   cwd: repo

  # Example sandbox profile that runs panes inside Docker.
  # sandbox:
  #   # Run panes inside a container instead of on the host.
  #   runtime: docker
  #   # Docker image used for the sandbox container.
  #   image: ghcr.io/your-org/your-image:latest
  #   # Forward selected host env vars into the container.
  #   envPassthrough:
  #     - ANTHROPIC_API_KEY
  #     - OPENAI_API_KEY
  #   # Extra system instructions for the agent in this profile.
  #   systemPrompt: >
  #     Extra instructions for the sandbox profile.
  #   # Skip agent permission prompts in this profile.
  #   yolo: true
  #   # Extra host paths to mount into the container.
  #   mounts:
  #     # Host path mounted into the sandbox.
  #     - hostPath: ~/.codex
  #       # Path inside the container.
  #       guestPath: /root/.codex
  #       # Allow writes through this mount.
  #       writable: true
  #   # Panes define the tmux layout created for sandbox sessions.
  #   panes:
  #     # Main AI coding pane.
  #     - id: agent
  #       # Pane type: agent, command, or shell.
  #       kind: agent
  #       # Focus this pane when the session opens.
  #       focus: true
  #     # Example shell pane for manual commands.
  #     - id: shell
  #       # Pane type: agent, command, or shell.
  #       kind: shell
  #       # Place this pane to the right of the existing layout.
  #       split: right
  #       # Start this pane in the repo root or managed worktree.
  #       cwd: repo

# Integrations connect Sebenza to external systems.
integrations:
  github:
    # Additional local repos Sebenza should consider alongside the main repo.
    linkedRepos:
      # GitHub slug for a related repo.
      # - repo: your-org/your-repo
      #   # Short label shown in the UI.
      #   alias: repo
      #   # Relative or absolute path to that local checkout.
      #   dir: ../your-repo
    # Remove managed worktrees automatically when their PR merges.
    # autoRemoveOnMerge: true

# startupEnvs become runtime env vars for panes, agents, and hooks.
startupEnvs:
  # Example feature flag available in every worktree session.
  # FEATURE_FLAG: true
  # Example service URL built from allocated ports.
  # API_BASE_URL: http://localhost:${PORT}

# lifecycleHooks run custom shell commands during worktree lifecycle events.
# lifecycleHooks:
#   # Runs after env setup and before panes start.
#   postCreate: bun install
#   # Runs before the worktree directory is removed.
#   preRemove: tmux kill-session -t "$SEBENZA_WORKTREE_ID" || true

# auto_name lets Sebenza generate a branch name when one is not provided.
# auto_name:
#   # Provider used for automatic branch naming.
#   provider: __DEFAULT_AGENT__
#   # Model used for automatic branch naming.
#   model: __AUTONAME_MODEL__
#   # Prompt that tells the model how to name branches.
#   system_prompt: >
#     Generate a short kebab-case git branch name.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> InitProjectContext {
        InitProjectContext {
            git_root: "/repo".to_string(),
            project_name: "MyApp".to_string(),
            main_branch: "trunk".to_string(),
            default_agent: InitAgent::Codex,
            package_manager: PackageManager::Pnpm,
        }
    }

    #[test]
    fn starter_template_substitutes_and_preserves_literal_port() {
        let out = build_starter_template(&ctx());
        assert!(out.contains("name: MyApp"));
        assert!(out.contains("mainBranch: trunk"));
        assert!(out.contains("defaultAgent: codex"));
        assert!(out.contains("PORT=$PORT pnpm dev"));
        assert!(out.contains("model: gpt-5.1-codex"));
        // Literal `${PORT}` / `${SEBENZA_WORKTREE_PATH}` must survive verbatim.
        assert!(out.contains("http://localhost:${PORT}"));
        assert!(out.contains("You are working in ${SEBENZA_WORKTREE_PATH}"));
        // No leftover substitution tokens.
        assert!(!out.contains("__"));
    }

    #[test]
    fn codex_command_has_summary_file_and_developer_instructions() {
        let prompt = build_init_prompt_spec(&ctx());
        let spec = build_init_agent_command(InitAgent::Codex, &prompt, "abc123");
        assert_eq!(spec.cmd, "codex");
        assert!(spec.summary_path.as_ref().unwrap().contains("sebenza-init-abc123"));
        assert!(spec.args.iter().any(|a| a.starts_with("developer_instructions=")));
    }
}
