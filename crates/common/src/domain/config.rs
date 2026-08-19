use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CustomAgentConfig {
    pub label: String,
    pub start_command: String,
    pub resume_command: Option<String>,
}

/// An external launcher (editor/tool) opened against a worktree directory via
/// the "Open in…" menu. `command` is a shell string with `${WORKTREE_PATH}`,
/// `${REPO_PATH}`, `${BRANCH}` template vars.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LauncherConfig {
    pub label: String,
    pub command: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PaneKind {
    Agent,
    Shell,
    Command,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PaneSplit {
    Right,
    Bottom,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PaneCwd {
    Worktree,
    Repo,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PaneTemplate {
    pub id: String,
    pub kind: PaneKind,
    pub split: Option<PaneSplit>,
    #[serde(rename = "sizePct")]
    pub size_pct: Option<i32>,
    pub focus: Option<bool>,
    pub command: Option<String>,
    pub cwd: Option<PaneCwd>,
    pub working_dir: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MountSpec {
    pub host_path: String,
    pub guest_path: Option<String>,
    pub writable: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeKind {
    Host,
    Docker,
    Lxc,
    Apple,
}

/// Host OS/arch/version bucket used to refuse a sandbox runtime early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPlatform {
    Linux,
    MacOsAppleSilicon26,
    Other,
}

/// A profile that cannot be launched on this host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRuntimeError {
    pub message: String,
    pub status: u16,
}

/// True for the live sandbox runtimes (not host, not the removed docker runtime).
pub fn is_sandboxed(runtime: RuntimeKind) -> bool {
    matches!(runtime, RuntimeKind::Lxc | RuntimeKind::Apple)
}

/// Returns a 422 when this profile must not be launched on `platform`.
///
/// `docker` is always refused (removed). `lxc`/`apple` need a non-empty `image`
/// and the matching OS. `host` is always ok.
pub fn profile_runtime_error(
    runtime: RuntimeKind,
    image: Option<&str>,
    platform: HostPlatform,
) -> Option<ProfileRuntimeError> {
    let image = image.map(str::trim).filter(|s| !s.is_empty());
    let err = |message: String| {
        Some(ProfileRuntimeError {
            message,
            status: 422,
        })
    };
    match runtime {
        RuntimeKind::Host => None,
        RuntimeKind::Docker => err(
            "The docker runtime has been removed. Use runtime: lxc on Linux or runtime: apple on macOS.".to_string(),
        ),
        RuntimeKind::Lxc => {
            if image.is_none() {
                return err("LXC profile is missing an image".to_string());
            }
            if platform != HostPlatform::Linux {
                return err("LXC runtime is only available on Linux.".to_string());
            }
            None
        }
        RuntimeKind::Apple => {
            if image.is_none() {
                return err("Apple profile is missing an image".to_string());
            }
            if platform != HostPlatform::MacOsAppleSilicon26 {
                return err(
                    "Apple runtime requires macOS 26 or later on Apple Silicon.".to_string(),
                );
            }
            None
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProfileConfig {
    pub runtime: RuntimeKind,
    pub system_prompt: Option<String>,
    pub env_passthrough: Vec<String>,
    pub yolo: Option<bool>,
    pub panes: Vec<PaneTemplate>,
    pub image: Option<String>,
    pub mounts: Option<Vec<MountSpec>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSpec {
    pub name: String,
    pub port_env: String,
    pub port_start: Option<u16>,
    pub port_step: Option<u16>,
    pub url_template: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LinkedRepoConfig {
    pub repo: String,
    pub alias: String,
    pub dir: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitHubIntegrationConfig {
    pub linked_repos: Vec<LinkedRepoConfig>,
    pub auto_remove_on_merge: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationConfig {
    pub github: GitHubIntegrationConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleHooksConfig {
    pub post_create: Option<String>,
    pub pre_remove: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AutoNameProvider {
    Claude,
    Codex,
    Opencode,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AutoNameConfig {
    pub provider: AutoNameProvider,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OneshotConfig {
    pub system_prompt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AutoPullConfig {
    pub enabled: bool,
    pub interval_seconds: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConfig {
    pub main_branch: String,
    pub worktree_root: String,
    pub default_agent: String, // "claude" | "codex"
    pub auto_pull: AutoPullConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub name: String,
    pub workspace: WorkspaceConfig,
    // IndexMap preserves YAML insertion order so `getDefaultProfileName` (first
    // key when no `default` profile) and the config profile list match the TS backend.
    pub profiles: IndexMap<String, ProfileConfig>,
    pub agents: HashMap<String, CustomAgentConfig>,
    pub launchers: HashMap<String, LauncherConfig>,
    pub services: Vec<ServiceSpec>,
    // Webmux supports boolean or string environment values. We deserialize into String to uniformize.
    pub startup_envs: HashMap<String, String>,
    pub integrations: IntegrationConfig,
    pub lifecycle_hooks: LifecycleHooksConfig,
    pub auto_name: Option<AutoNameConfig>,
    pub oneshot: OneshotConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_yaml(runtime: &str) -> String {
        format!("runtime: {runtime}\nenvPassthrough: []\npanes: []\n")
    }

    #[test]
    fn profile_config_deserializes_lxc_and_apple_and_still_accepts_docker() {
        let lxc: ProfileConfig = serde_yaml::from_str(&profile_yaml("lxc")).unwrap();
        assert_eq!(lxc.runtime, RuntimeKind::Lxc);

        let apple: ProfileConfig = serde_yaml::from_str(&profile_yaml("apple")).unwrap();
        assert_eq!(apple.runtime, RuntimeKind::Apple);

        let docker: ProfileConfig = serde_yaml::from_str(&profile_yaml("docker")).unwrap();
        assert_eq!(docker.runtime, RuntimeKind::Docker);

        let host: ProfileConfig = serde_yaml::from_str(&profile_yaml("host")).unwrap();
        assert_eq!(host.runtime, RuntimeKind::Host);
    }

    #[test]
    fn docker_runtime_is_refused_even_when_an_image_is_set() {
        let err = profile_runtime_error(RuntimeKind::Docker, Some("example"), HostPlatform::Linux)
            .expect("docker must be refused");
        assert_eq!(err.status, 422);
        let msg = err.message.to_lowercase();
        assert!(msg.contains("removed"), "{msg}");
        assert!(msg.contains("lxc"), "{msg}");
        assert!(msg.contains("apple"), "{msg}");
    }

    #[test]
    fn lxc_and_apple_require_an_image() {
        let lxc = profile_runtime_error(RuntimeKind::Lxc, None, HostPlatform::Linux)
            .expect("lxc needs an image");
        assert_eq!(lxc.status, 422);
        assert!(
            lxc.message.to_lowercase().contains("image"),
            "{}",
            lxc.message
        );

        let apple =
            profile_runtime_error(RuntimeKind::Apple, None, HostPlatform::MacOsAppleSilicon26)
                .expect("apple needs an image");
        assert_eq!(apple.status, 422);
        assert!(
            apple.message.to_lowercase().contains("image"),
            "{}",
            apple.message
        );
    }

    #[test]
    fn lxc_and_apple_are_refused_on_the_wrong_platform() {
        let lxc =
            profile_runtime_error(RuntimeKind::Lxc, Some("ubuntu:24.04"), HostPlatform::Other)
                .expect("lxc is linux-only");
        assert_eq!(lxc.status, 422);
        assert!(
            lxc.message.to_lowercase().contains("linux"),
            "{}",
            lxc.message
        );

        let apple = profile_runtime_error(
            RuntimeKind::Apple,
            Some("ubuntu:24.04"),
            HostPlatform::Linux,
        )
        .expect("apple is macos-only");
        assert_eq!(apple.status, 422);
        let msg = apple.message.to_lowercase();
        assert!(msg.contains("macos"), "{msg}");
        assert!(
            msg.contains("apple silicon") || msg.contains("apple-silicon"),
            "{msg}"
        );
    }

    #[test]
    fn host_and_matching_sandbox_profiles_are_ok() {
        assert!(profile_runtime_error(RuntimeKind::Host, None, HostPlatform::Linux).is_none());
        assert!(
            profile_runtime_error(RuntimeKind::Lxc, Some("ubuntu:24.04"), HostPlatform::Linux)
                .is_none()
        );
        assert!(
            profile_runtime_error(
                RuntimeKind::Apple,
                Some("ubuntu:24.04"),
                HostPlatform::MacOsAppleSilicon26,
            )
            .is_none()
        );
    }

    #[test]
    fn is_sandboxed_is_lxc_or_apple_only() {
        assert!(!is_sandboxed(RuntimeKind::Host));
        assert!(!is_sandboxed(RuntimeKind::Docker));
        assert!(is_sandboxed(RuntimeKind::Lxc));
        assert!(is_sandboxed(RuntimeKind::Apple));
    }
}
