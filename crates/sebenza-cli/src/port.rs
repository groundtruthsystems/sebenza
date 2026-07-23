//! Live-server port discovery. Server-backed commands talk to a running
//! `sebenza-server`, whose port is whatever it bound at startup — not necessarily
//! the 5111 default (it walks to a free port when 5111 is taken). Matching the
//! instance registry by project dir lets those commands find the right server.

use common::adapters::instance_registry::{self, InstanceEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortSource {
    Project,
    Sole,
    Default,
}

impl PortSource {
    pub fn as_str(self) -> &'static str {
        match self {
            PortSource::Project => "project",
            PortSource::Sole => "sole",
            PortSource::Default => "default",
        }
    }
}

pub struct ResolvedPort {
    pub port: u16,
    pub source: PortSource,
}

fn is_inside(child: &str, parent: &str) -> bool {
    let root = parent.strip_suffix('/').unwrap_or(parent);
    child == root || child.starts_with(&format!("{root}/"))
}

/// Pick the port of the live instance that serves the current project. Falls
/// back to the sole live instance when nothing matches (the common
/// single-instance setup), then to `default_port`.
pub fn select_instance_port(
    default_port: u16,
    candidate_dirs: &[String],
    instances: &[InstanceEntry],
) -> ResolvedPort {
    if let Some(m) = instances
        .iter()
        .find(|entry| candidate_dirs.iter().any(|dir| is_inside(dir, &entry.project_dir)))
    {
        return ResolvedPort { port: m.port, source: PortSource::Project };
    }
    if instances.len() == 1 {
        return ResolvedPort { port: instances[0].port, source: PortSource::Sole };
    }
    ResolvedPort { port: default_port, source: PortSource::Default }
}

/// Read the live registry + resolve the current project's git root, then
/// delegate to `select_instance_port`.
pub fn resolve_live_server_port(default_port: u16, cwd: &str) -> ResolvedPort {
    let instances = instance_registry::list_live();
    let mut candidate_dirs = vec![cwd.to_string()];
    if let Some(root) = crate::http::resolve_project_root(cwd) {
        if root != cwd {
            candidate_dirs.push(root);
        }
    }
    select_instance_port(default_port, &candidate_dirs, &instances)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(port: u16, dir: &str) -> InstanceEntry {
        InstanceEntry { port, project_dir: dir.to_string(), pid: 0 }
    }

    #[test]
    fn matches_project_dir_when_cwd_is_inside() {
        let instances = vec![entry(5112, "/home/me/repo"), entry(5113, "/home/me/other")];
        let got = select_instance_port(5111, &["/home/me/repo/sub".into()], &instances);
        assert_eq!(got.port, 5112);
        assert_eq!(got.source, PortSource::Project);
    }

    #[test]
    fn falls_back_to_sole_instance() {
        let instances = vec![entry(5115, "/somewhere/else")];
        let got = select_instance_port(5111, &["/home/me/repo".into()], &instances);
        assert_eq!(got.port, 5115);
        assert_eq!(got.source, PortSource::Sole);
    }

    #[test]
    fn falls_back_to_default_when_ambiguous() {
        let instances = vec![entry(5112, "/a"), entry(5113, "/b")];
        let got = select_instance_port(5111, &["/home/me/repo".into()], &instances);
        assert_eq!(got.port, 5111);
        assert_eq!(got.source, PortSource::Default);
    }
}
