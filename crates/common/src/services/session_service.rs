use crate::adapters::tmux::{TmuxGateway, build_project_session_name, build_worktree_window_name};
use crate::domain::config::{PaneCwd, PaneKind, PaneSplit, PaneTemplate};
use std::path::Path;

/// Startup commands for the agent/shell pane kinds (built by the lifecycle
/// service from the resolved profile + agent binary).
#[derive(Clone)]
pub struct PaneCommandSet {
    pub agent: String,
    pub shell: String,
}

pub struct SessionLayoutContext {
    pub repo_root: String,
    pub worktree_path: String,
    pub pane_commands: PaneCommandSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedPane {
    pub id: String,
    pub index: usize,
    pub kind: PaneKind,
    pub cwd: String,
    pub startup_command: Option<String>,
    pub focus: bool,
    pub split: Option<PaneSplit>,
    pub size_pct: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLayoutPlan {
    pub session_name: String,
    pub window_name: String,
    pub shell_command: String,
    pub panes: Vec<PlannedPane>,
    pub focus_pane_index: usize,
}

fn quote_shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn resolve_pane_cwd(template: &PaneTemplate, ctx: &SessionLayoutContext) -> String {
    match template.cwd {
        Some(PaneCwd::Repo) => ctx.repo_root.clone(),
        _ => ctx.worktree_path.clone(),
    }
}

fn build_command_pane_startup_command(
    template: &PaneTemplate,
    ctx: &SessionLayoutContext,
) -> Result<String, String> {
    let command = template.command.as_ref().ok_or_else(|| {
        format!(
            "Pane \"{}\" is kind=command but has no command",
            template.id
        )
    })?;
    let Some(working_dir) = &template.working_dir else {
        return Ok(command.clone());
    };
    let base = resolve_pane_cwd(template, ctx);
    let resolved = Path::new(&base).join(working_dir);
    Ok(format!(
        "cd -- {} && {command}",
        quote_shell(&resolved.to_string_lossy())
    ))
}

fn resolve_pane_startup_command(
    template: &PaneTemplate,
    ctx: &SessionLayoutContext,
) -> Result<Option<String>, String> {
    match template.kind {
        PaneKind::Agent => Ok(Some(ctx.pane_commands.agent.clone())),
        PaneKind::Shell => Ok(None),
        PaneKind::Command => Ok(Some(build_command_pane_startup_command(template, ctx)?)),
    }
}

pub fn plan_session_layout(
    project_root: &str,
    branch: &str,
    templates: &[PaneTemplate],
    ctx: &SessionLayoutContext,
) -> Result<SessionLayoutPlan, String> {
    if templates.is_empty() {
        return Err("At least one pane template is required".to_string());
    }

    let mut panes = Vec::with_capacity(templates.len());
    for (index, template) in templates.iter().enumerate() {
        let startup_command = resolve_pane_startup_command(template, ctx)?;
        let (split, size_pct) = if index > 0 {
            (
                Some(template.split.unwrap_or(PaneSplit::Right)),
                template.size_pct,
            )
        } else {
            (None, None)
        };
        panes.push(PlannedPane {
            id: template.id.clone(),
            index,
            kind: template.kind,
            cwd: resolve_pane_cwd(template, ctx),
            startup_command,
            focus: template.focus == Some(true),
            split,
            size_pct,
        });
    }

    let focus_pane_index = panes.iter().find(|p| p.focus).map(|p| p.index).unwrap_or(0);

    Ok(SessionLayoutPlan {
        session_name: build_project_session_name(project_root),
        window_name: build_worktree_window_name(branch),
        shell_command: ctx.pane_commands.shell.clone(),
        panes,
        focus_pane_index,
    })
}

pub fn is_worktree_open(tmux: &TmuxGateway, project_root: &str, branch: &str) -> bool {
    let session_name = build_project_session_name(project_root);
    let window_name = build_worktree_window_name(branch);
    tmux.has_window(&session_name, &window_name)
}

/// Build (or rebuild) the worktree's tmux window from a plan: ensure the session,
/// create the window + splits, run pane startup commands, and focus.
pub fn ensure_session_layout(tmux: &TmuxGateway, plan: &SessionLayoutPlan) -> Result<(), String> {
    let root_pane = &plan.panes[0];
    tmux.ensure_server()?;
    tmux.ensure_session(&plan.session_name, &root_pane.cwd)?;

    if tmux.has_window(&plan.session_name, &plan.window_name) {
        tmux.kill_window(&plan.session_name, &plan.window_name)?;
    }

    tmux.create_window(
        &plan.session_name,
        &plan.window_name,
        &root_pane.cwd,
        Some(&plan.shell_command),
    )?;
    tmux.set_window_option(
        &plan.session_name,
        &plan.window_name,
        "pane-base-index",
        "0",
    )?;
    tmux.set_window_option(
        &plan.session_name,
        &plan.window_name,
        "automatic-rename",
        "off",
    )?;
    tmux.set_window_option(&plan.session_name, &plan.window_name, "allow-rename", "off")?;

    for pane in &plan.panes[1..] {
        let target = format!(
            "{}:{}.{}",
            plan.session_name,
            plan.window_name,
            pane.index - 1
        );
        tmux.split_window(
            &target,
            pane.split.unwrap_or(PaneSplit::Right),
            pane.size_pct,
            &pane.cwd,
            Some(&plan.shell_command),
        )?;
    }

    for pane in &plan.panes {
        let Some(command) = &pane.startup_command else {
            continue;
        };
        let target = format!("{}:{}.{}", plan.session_name, plan.window_name, pane.index);
        tmux.run_command(&target, command)?;
    }

    tmux.select_pane(&format!(
        "{}:{}.{}",
        plan.session_name, plan.window_name, plan.focus_pane_index
    ))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(id: &str, kind: PaneKind) -> PaneTemplate {
        PaneTemplate {
            id: id.to_string(),
            kind,
            split: None,
            size_pct: None,
            focus: None,
            command: None,
            cwd: None,
            working_dir: None,
        }
    }

    fn ctx() -> SessionLayoutContext {
        SessionLayoutContext {
            repo_root: "/repo".to_string(),
            worktree_path: "/repo/__worktrees/feature".to_string(),
            pane_commands: PaneCommandSet {
                agent: "claude".to_string(),
                shell: "bash".to_string(),
            },
        }
    }

    #[test]
    fn plans_agent_and_shell_panes_with_splits() {
        let mut agent = template("agent", PaneKind::Agent);
        agent.focus = Some(true);
        let mut shell = template("shell", PaneKind::Shell);
        shell.split = Some(PaneSplit::Bottom);
        shell.size_pct = Some(35);

        let plan = plan_session_layout("/repo", "feature", &[agent, shell], &ctx()).unwrap();

        assert_eq!(plan.window_name, "sebenza-feature");
        assert_eq!(plan.shell_command, "bash");
        assert_eq!(plan.focus_pane_index, 0);
        // Pane 0 (agent): no split, agent startup command, cwd = worktree.
        assert_eq!(plan.panes[0].split, None);
        assert_eq!(plan.panes[0].startup_command.as_deref(), Some("claude"));
        assert_eq!(plan.panes[0].cwd, "/repo/__worktrees/feature");
        // Pane 1 (shell): bottom split, no startup command.
        assert_eq!(plan.panes[1].split, Some(PaneSplit::Bottom));
        assert_eq!(plan.panes[1].size_pct, Some(35));
        assert_eq!(plan.panes[1].startup_command, None);
    }

    #[test]
    fn command_pane_prefixes_cd_when_working_dir_set() {
        let mut cmd = template("fe", PaneKind::Command);
        cmd.command = Some("npm run dev".to_string());
        cmd.working_dir = Some("frontend".to_string());
        cmd.cwd = Some(PaneCwd::Repo);
        let agent = {
            let mut a = template("agent", PaneKind::Agent);
            a.focus = Some(true);
            a
        };

        let plan = plan_session_layout("/repo", "feature", &[agent, cmd], &ctx()).unwrap();
        assert_eq!(
            plan.panes[1].startup_command.as_deref(),
            Some("cd -- '/repo/frontend' && npm run dev")
        );
    }

    #[test]
    fn rejects_empty_template_list() {
        assert!(plan_session_layout("/repo", "feature", &[], &ctx()).is_err());
    }

    #[test]
    fn command_pane_without_command_errors() {
        let agent = template("agent", PaneKind::Agent);
        let bad = template("cmd", PaneKind::Command);
        let err = plan_session_layout("/repo", "feature", &[agent, bad], &ctx()).unwrap_err();
        assert!(err.contains("has no command"));
    }

    /// The context the main-repo open path builds: worktree == repo, and an empty
    /// agent command because no agent pane is planned.
    fn repo_ctx() -> SessionLayoutContext {
        SessionLayoutContext {
            repo_root: "/repo".to_string(),
            worktree_path: "/repo".to_string(),
            pane_commands: PaneCommandSet {
                agent: String::new(),
                shell: "managed-shell".to_string(),
            },
        }
    }

    #[test]
    fn repo_shell_plan_has_a_single_shell_pane_at_the_repo_root() {
        let mut shell = template("shell", PaneKind::Shell);
        shell.focus = Some(true);
        let plan = plan_session_layout("/repo", "main", &[shell], &repo_ctx()).unwrap();

        assert_eq!(plan.panes.len(), 1);
        assert_eq!(plan.panes[0].kind, PaneKind::Shell);
        assert_eq!(plan.panes[0].cwd, "/repo");
        assert_eq!(plan.panes[0].split, None);
        assert_eq!(plan.focus_pane_index, 0);
        // A shell pane is created *with* the window, so it has no typed command.
        assert_eq!(plan.panes[0].startup_command, None);
    }

    #[test]
    fn repo_shell_plan_ignores_the_agent_command() {
        // Locks in why the main-repo path may pass an empty agent command:
        // `resolve_pane_startup_command` never reads it for a shell pane. If that
        // stops holding, this fails rather than silently planning `sh -c ''`.
        let plan = plan_session_layout(
            "/repo",
            "main",
            &[template("shell", PaneKind::Shell)],
            &repo_ctx(),
        )
        .unwrap();
        assert_eq!(plan.panes[0].startup_command, None);
        assert_eq!(plan.shell_command, "managed-shell");
    }

    #[test]
    fn repo_shell_plan_window_name_is_the_main_branch() {
        let plan = plan_session_layout(
            "/repo",
            "main",
            &[template("shell", PaneKind::Shell)],
            &repo_ctx(),
        )
        .unwrap();
        assert_eq!(plan.window_name, build_worktree_window_name("main"));
        assert_eq!(plan.window_name, "sebenza-main");
        // Same tmux session as every worktree in this project — only the window differs.
        assert_eq!(plan.session_name, build_project_session_name("/repo"));
    }
}
