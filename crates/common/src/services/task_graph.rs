//! Builds an inbox item's [`TaskGraph`] from its `conversions[]` and each
//! worktree's `tracks.json` / `plan.json`.
//!
//! Read-only and per request: nothing is cached or written. Every path comes
//! from untrusted places (the item's frontmatter, repo-controlled
//! `tracks.json`), so reads are confined:
//!
//! - a worktree must canonicalise to a directory the caller's
//!   [`WorktreeRegistry`] knows, as well as being recorded on this item;
//! - only fixed names are opened under it (`.ai/sebenza/tracks.json`,
//!   `.ai/sebenza/tracks/<track_id>/plan.json`), with ids checked by
//!   [`is_plan_id`] before they touch a path and `plan_path` ignored;
//! - no component under the worktree may be a symlink, the file itself is
//!   opened `O_NOFOLLOW`, and each file is capped at [`MAX_FILE_BYTES`];
//! - failures become fixed error codes, never paths or file content.

use crate::domain::inbox_events::RequestView;
use crate::domain::task_graph::{
    AgentState, DepRef, EdgeKind, GraphEdge, PhaseNode, Progress, TaskGraph, TaskNode, TaskRequest,
    TrackNode, WorktreeNode, WorktreeState, is_plan_id,
};
use crate::services::inbox_convert::ConversionOutcome;
use std::path::{Path, PathBuf};

/// Largest `tracks.json` / `plan.json` the graph will read.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Worktrees shown in full; later ones are counted, not read.
pub const MAX_WORKTREES: usize = 25;
/// Task nodes across the whole graph.
pub const MAX_TASK_NODES: usize = 2000;

#[cfg(target_os = "linux")]
const O_NOFOLLOW: i32 = 0o400000;
#[cfg(not(target_os = "linux"))]
const O_NOFOLLOW: i32 = 0x0100;

/// Which canonical directories are worktrees the daemon itself manages.
pub trait WorktreeRegistry {
    fn is_registered(&self, project_path: &str, canonical: &Path) -> bool;
}

/// The live agent of a worktree, if the daemon has one in memory.
pub trait AgentStateLookup {
    fn agent_state(&self, project_path: &str, branch: &str) -> Option<AgentState>;
}

/// What the graph is built from.
pub struct GraphInput<'a> {
    pub draft_id: &'a str,
    pub conversions: &'a [serde_yaml::Value],
    pub requests: &'a [RequestView],
    pub now: String,
}

/// Build the graph. Never fails: what cannot be read is a state or code.
pub fn build_task_graph(
    input: &GraphInput<'_>,
    registry: &dyn WorktreeRegistry,
    agents: &dyn AgentStateLookup,
) -> TaskGraph {
    let mut graph = TaskGraph {
        draft_id: input.draft_id.to_string(),
        generated_at: input.now.clone(),
        etag: graph_etag(input, registry),
        truncated: false,
        progress: Progress::default(),
        failed: 0,
        unavailable: 0,
        worktrees: Vec::new(),
        edges: Vec::new(),
    };
    let mut task_budget = MAX_TASK_NODES;
    for (i, raw) in input.conversions.iter().enumerate() {
        let node = worktree_node(
            i,
            raw,
            input,
            registry,
            agents,
            &mut task_budget,
            &mut graph.truncated,
        );
        match node.state {
            WorktreeState::Ok => graph.progress.add(node.progress),
            WorktreeState::Failed => graph.failed += 1,
            WorktreeState::Unavailable => graph.unavailable += 1,
            WorktreeState::NoWorkspace => {}
        }
        graph.worktrees.push(node);
    }
    graph.edges = edges(&graph.worktrees);
    graph
}

/// A cheap fingerprint of everything the graph would read: the conversion
/// records, the requests' states and each worktree file's (path, mtime,
/// size). Equal fingerprints mean an unchanged graph, so a poll can be
/// answered 304 without parsing anything.
pub fn graph_etag(input: &GraphInput<'_>, registry: &dyn WorktreeRegistry) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(input.draft_id.as_bytes());
    for raw in input.conversions {
        h.update(serde_yaml::to_string(raw).unwrap_or_default().as_bytes());
        let Some(c) = conversion(raw) else { continue };
        let (Some(path), true) = (c.worktree_path.as_deref(), c.outcome == "created") else {
            continue;
        };
        let Ok(root) = confine(path, &c.project_path, registry) else {
            continue;
        };
        stamp(&mut h, &root.join(".ai/sebenza/tracks.json"));
        if let Ok(entries) = std::fs::read_dir(root.join(".ai/sebenza/tracks")) {
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| is_plan_id(n))
                .collect();
            names.sort();
            for n in names {
                h.update(n.as_bytes());
                stamp(
                    &mut h,
                    &root.join(".ai/sebenza/tracks").join(&n).join("plan.json"),
                );
            }
        }
    }
    for r in input.requests {
        h.update(r.request_id.as_bytes());
        h.update(format!("{:?}{:?}", r.status, r.task).as_bytes());
    }
    hex::encode(h.finalize())
}

fn stamp(h: &mut sha1::Sha1, path: &Path) {
    use sha1::Digest;
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        h.update(format!("{}:{}:{}", path.display(), mtime, meta.len()).as_bytes());
    }
}

fn conversion(raw: &serde_yaml::Value) -> Option<ConversionOutcome> {
    serde_yaml::from_value(raw.clone()).ok()
}

/// The canonical worktree root, if it exists and the registry vouches for it.
fn confine(
    worktree_path: &str,
    project_path: &str,
    registry: &dyn WorktreeRegistry,
) -> Result<PathBuf, &'static str> {
    let canonical = std::fs::canonicalize(worktree_path).map_err(|_| "missing")?;
    if !canonical.is_dir() {
        return Err("missing");
    }
    if !registry.is_registered(project_path, &canonical) {
        return Err("not_registered");
    }
    Ok(canonical)
}

/// Read `root/<rel...>` refusing any symlink on the way and anything larger
/// than [`MAX_FILE_BYTES`]. `rel` components are fixed names or ids that have
/// passed [`is_plan_id`].
fn safe_read(root: &Path, rel: &[&str]) -> Result<String, &'static str> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let (file, dirs) = rel.split_last().ok_or("missing")?;
    let mut cur = root.to_path_buf();
    for d in dirs {
        cur.push(d);
        let meta = match std::fs::symlink_metadata(&cur) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err("missing"),
            Err(_) => return Err("unreadable"),
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err("unreadable");
        }
    }
    cur.push(file);
    let meta = match std::fs::symlink_metadata(&cur) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err("missing"),
        Err(_) => return Err("unreadable"),
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("unreadable");
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err("too_large");
    }
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(&cur)
        .map_err(|_| "unreadable")?;
    let mut text = String::new();
    f.take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| "unreadable")?;
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err("too_large");
    }
    Ok(text)
}

fn read_json(root: &Path, rel: &[&str]) -> Result<serde_json::Value, &'static str> {
    let text = safe_read(root, rel)?;
    serde_json::from_str(&text).map_err(|_| "invalid_json")
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn worktree_node(
    i: usize,
    raw: &serde_yaml::Value,
    input: &GraphInput<'_>,
    registry: &dyn WorktreeRegistry,
    agents: &dyn AgentStateLookup,
    task_budget: &mut usize,
    truncated: &mut bool,
) -> WorktreeNode {
    let c = conversion(raw);
    let (branch, project) = c
        .as_ref()
        .map(|c| (c.branch.clone(), c.project_path.clone()))
        .unwrap_or_default();
    let mut node = WorktreeNode {
        id: format!("w:{i}"),
        branch: branch.clone(),
        project_path: project.clone(),
        state: WorktreeState::Unavailable,
        error_code: None,
        progress: Progress::default(),
        agent: None,
        tracks: Vec::new(),
    };
    let Some(c) = c else {
        node.error_code = Some("unreadable");
        return node;
    };
    if c.outcome != "created" {
        node.state = WorktreeState::Failed;
        node.error_code = Some("conversion_failed");
        return node;
    }
    node.agent = agents.agent_state(&project, &branch);
    if i >= MAX_WORKTREES {
        *truncated = true;
        node.error_code = Some("over_cap");
        return node;
    }
    let root = match c
        .worktree_path
        .as_deref()
        .ok_or("missing")
        .and_then(|p| confine(p, &project, registry))
    {
        Ok(root) => root,
        Err(code) => {
            node.error_code = Some(code);
            return node;
        }
    };
    let tracks = match read_json(&root, &[".ai", "sebenza", "tracks.json"]) {
        Ok(v) => v,
        Err(code) => {
            node.state = WorktreeState::NoWorkspace;
            node.error_code = Some(code);
            return node;
        }
    };
    node.state = WorktreeState::Ok;
    let list = tracks
        .get("tracks")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (n, entry) in list.iter().enumerate() {
        let track = track_node(
            i,
            n,
            entry,
            &root,
            &branch,
            &project,
            input.requests,
            task_budget,
            truncated,
        );
        node.progress.add(track.progress);
        node.tracks.push(track);
    }
    node
}

#[allow(clippy::too_many_arguments)]
fn track_node(
    i: usize,
    n: usize,
    entry: &serde_json::Value,
    root: &Path,
    branch: &str,
    project: &str,
    requests: &[RequestView],
    task_budget: &mut usize,
    truncated: &mut bool,
) -> TrackNode {
    let track_id = str_field(entry, "track_id");
    let mut node = TrackNode {
        id: format!("t:{i}/{track_id}"),
        track_id: track_id.clone(),
        description: str_field(entry, "description"),
        status: str_field(entry, "status"),
        current_phase: None,
        designing: false,
        error_code: None,
        progress: Progress::default(),
        phases: Vec::new(),
    };
    if !is_plan_id(&track_id) {
        // Never let a bad id near a path, or into a node id.
        node.id = format!("t:{i}/#{n}");
        node.track_id = String::new();
        node.error_code = Some("invalid_id");
        return node;
    }
    let plan = match read_json(root, &[".ai", "sebenza", "tracks", &track_id, "plan.json"]) {
        Ok(p) => p,
        Err("missing") => {
            node.designing = true;
            return node;
        }
        Err(code) => {
            node.error_code = Some(code);
            return node;
        }
    };
    let phases = plan
        .get("phases")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for phase in &phases {
        let phase_id = str_field(phase, "id");
        if !is_plan_id(&phase_id) {
            continue;
        }
        let mut p = PhaseNode {
            id: format!("p:{i}/{track_id}/{phase_id}"),
            phase_id: phase_id.clone(),
            name: str_field(phase, "name"),
            status: str_field(phase, "status"),
            progress: Progress::default(),
            tasks: Vec::new(),
        };
        for task in phase
            .get("tasks")
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let task_id = str_field(task, "id");
            if !is_plan_id(&task_id) {
                continue;
            }
            let status = str_field(task, "status");
            p.progress.total += 1;
            if status == "done" {
                p.progress.done += 1;
            }
            if *task_budget == 0 {
                *truncated = true;
                continue;
            }
            *task_budget -= 1;
            let depends_on = task
                .get("depends_on")
                .and_then(serde_json::Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(serde_json::Value::as_str)
                        .take(50)
                        .map(|s| s.chars().take(200).collect())
                        .collect()
                })
                .unwrap_or_default();
            let request = inbox_request(requests, project, branch, &track_id, &task_id)
                .or_else(|| plan_request(task));
            p.tasks.push(TaskNode {
                id: format!("k:{i}/{track_id}/{task_id}"),
                task_id,
                name: str_field(task, "name"),
                status,
                depends_on,
                request,
            });
        }
        node.progress.add(p.progress);
        node.phases.push(p);
    }
    let named = str_field(entry, "current_phase_id");
    node.current_phase = node
        .phases
        .iter()
        .find(|p| p.phase_id == named)
        .or_else(|| {
            node.phases
                .iter()
                .find(|p| matches!(p.status.as_str(), "doing" | "blocked" | "unblocked"))
        })
        .or_else(|| node.phases.iter().find(|p| p.status != "done"))
        .map(|p| p.phase_id.clone());
    node
}

fn inbox_request(
    requests: &[RequestView],
    project: &str,
    branch: &str,
    track_id: &str,
    task_id: &str,
) -> Option<TaskRequest> {
    requests
        .iter()
        .rev()
        .find(|r| {
            r.worktree.project == project
                && r.worktree.branch == branch
                && r.task
                    .as_ref()
                    .is_some_and(|t| t.track_id == track_id && t.task_id == task_id)
        })
        .map(|r| TaskRequest {
            request_id: Some(r.request_id.clone()),
            status: serde_json::to_value(r.status)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default(),
            source: "inbox",
        })
}

fn plan_request(task: &serde_json::Value) -> Option<TaskRequest> {
    let r = task.get("request")?;
    let status = str_field(r, "status");
    if status.is_empty() {
        return None;
    }
    Some(TaskRequest {
        request_id: r
            .get("request_id")
            .and_then(serde_json::Value::as_str)
            .filter(|s| is_plan_id(s))
            .map(str::to_string),
        status,
        source: "plan",
    })
}

/// A dangling target's node id: the raw ref, reduced to safe characters.
fn dangling_id(raw: &str) -> String {
    let clean: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "_.-/:@".contains(*c))
        .take(120)
        .collect();
    format!("x:{clean}")
}

fn edges(worktrees: &[WorktreeNode]) -> Vec<GraphEdge> {
    use std::collections::{HashMap, HashSet};
    let contains = |from: &str, to: &str| GraphEdge {
        from: from.to_string(),
        to: to.to_string(),
        kind: EdgeKind::Contains,
        dangling: false,
        flag: None,
    };
    let mut out = Vec::new();
    let mut tasks: HashSet<String> = HashSet::new();
    let mut by_branch: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, w) in worktrees.iter().enumerate() {
        if w.state != WorktreeState::Failed {
            by_branch.entry(w.branch.as_str()).or_default().push(i);
        }
        for t in &w.tracks {
            out.push(contains(&w.id, &t.id));
            for p in &t.phases {
                out.push(contains(&t.id, &p.id));
                for k in &p.tasks {
                    out.push(contains(&p.id, &k.id));
                    tasks.insert(k.id.clone());
                }
            }
        }
    }
    let first_dep = out.len();
    for (i, w) in worktrees.iter().enumerate() {
        for t in &w.tracks {
            for p in &t.phases {
                for k in &p.tasks {
                    for raw in &k.depends_on {
                        out.push(dependency(i, &t.track_id, &k.id, raw, &by_branch, &tasks));
                    }
                }
            }
        }
    }
    flag_cycles(&mut out[first_dep..]);
    out
}

fn dependency(
    i: usize,
    track_id: &str,
    from: &str,
    raw: &str,
    by_branch: &std::collections::HashMap<&str, Vec<usize>>,
    tasks: &std::collections::HashSet<String>,
) -> GraphEdge {
    let dangling = |flag: &'static str| GraphEdge {
        from: from.to_string(),
        to: dangling_id(raw),
        kind: EdgeKind::DependsOn,
        dangling: true,
        flag: Some(flag),
    };
    let Some(r) = DepRef::parse(raw) else {
        return dangling("malformed");
    };
    let target_wt = match r.branch.as_deref() {
        None => i,
        Some(b) => match by_branch.get(b).map(Vec::as_slice) {
            Some([only]) => *only,
            Some([_, _, ..]) => return dangling("ambiguous"),
            _ => return dangling("unknown"),
        },
    };
    let track = r.track_id.as_deref().unwrap_or(track_id);
    let to = format!("k:{target_wt}/{track}/{}", r.task_id);
    if !tasks.contains(&to) {
        return dangling("unknown");
    }
    GraphEdge {
        from: from.to_string(),
        to,
        kind: EdgeKind::DependsOn,
        dangling: false,
        flag: None,
    }
}

/// Flag every resolved dependency edge that lies on a cycle. Iterative, so a
/// hostile plan cannot recurse the stack.
fn flag_cycles(deps: &mut [GraphEdge]) {
    use std::collections::{HashMap, HashSet};
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in deps.iter().filter(|e| !e.dangling) {
        adj.entry(e.from.as_str()).or_default().push(e.to.as_str());
    }
    let reaches = |start: &str, goal: &str| -> bool {
        let mut stack = vec![start];
        let mut seen: HashSet<&str> = HashSet::new();
        while let Some(n) = stack.pop() {
            if n == goal {
                return true;
            }
            if seen.insert(n) {
                stack.extend(adj.get(n).into_iter().flatten().copied());
            }
        }
        false
    };
    let on_cycle: Vec<bool> = deps
        .iter()
        .map(|e| !e.dangling && reaches(&e.to, &e.from))
        .collect();
    for (e, cyclic) in deps.iter_mut().zip(on_cycle) {
        if cyclic {
            e.flag = Some("cycle");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::inbox_events::{RequestStatus, TaskLink, WorktreeKey};
    use crate::services::inbox_convert::ConversionTarget;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    fn temp(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let d =
            std::env::temp_dir().join(format!("sebenza-graph-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    /// Registers every directory it is told about.
    #[derive(Default)]
    struct Known(Vec<PathBuf>);
    impl WorktreeRegistry for Known {
        fn is_registered(&self, _project: &str, canonical: &Path) -> bool {
            self.0.iter().any(|p| p == canonical)
        }
    }

    #[derive(Default)]
    struct Agents(HashMap<String, AgentState>);
    impl AgentStateLookup for Agents {
        fn agent_state(&self, _project: &str, branch: &str) -> Option<AgentState> {
            self.0.get(branch).cloned()
        }
    }

    fn created(project: &str, branch: &str, path: &Path) -> serde_yaml::Value {
        let t = ConversionTarget {
            project_path: project.into(),
            branch: branch.into(),
            ..Default::default()
        };
        serde_yaml::to_value(ConversionOutcome::created(
            &t,
            path.to_string_lossy().to_string(),
            "2026-11-05T00:00:00Z".into(),
        ))
        .unwrap()
    }

    fn failed(project: &str, branch: &str) -> serde_yaml::Value {
        let t = ConversionTarget {
            project_path: project.into(),
            branch: branch.into(),
            ..Default::default()
        };
        serde_yaml::to_value(ConversionOutcome::failed(
            &t,
            "boom at /home/secret/path".into(),
            "2026-11-05T00:00:00Z".into(),
        ))
        .unwrap()
    }

    /// A worktree with a Sebenza workspace holding `tracks` (id, description,
    /// optional plan JSON).
    fn worktree(root: &Path, name: &str, tracks: &[(&str, Option<serde_json::Value>)]) -> PathBuf {
        let wt = root.join(name);
        let ws = wt.join(".ai/sebenza");
        std::fs::create_dir_all(&ws).unwrap();
        let entries: Vec<serde_json::Value> = tracks
            .iter()
            .map(|(id, _)| serde_json::json!({"track_id": id, "description": format!("{id} desc"), "status": "doing"}))
            .collect();
        std::fs::write(
            ws.join("tracks.json"),
            serde_json::json!({"tracks": entries}).to_string(),
        )
        .unwrap();
        for (id, plan) in tracks {
            if let Some(plan) = plan {
                std::fs::create_dir_all(ws.join("tracks").join(id)).unwrap();
                std::fs::write(
                    ws.join("tracks").join(id).join("plan.json"),
                    plan.to_string(),
                )
                .unwrap();
            }
        }
        wt
    }

    fn task(id: &str, status: &str, deps: &[&str]) -> serde_json::Value {
        serde_json::json!({"id": id, "name": format!("{id} name"), "status": status, "depends_on": deps})
    }

    fn plan(phases: Vec<(&str, &str, Vec<serde_json::Value>)>) -> serde_json::Value {
        serde_json::json!({
            "$schema": "sebenza-plan-v1",
            "track_id": "t",
            "phases": phases.into_iter().map(|(id, status, tasks)| serde_json::json!({
                "id": id, "name": format!("{id} name"), "status": status, "tasks": tasks
            })).collect::<Vec<_>>()
        })
    }

    fn graph(conversions: &[serde_yaml::Value], known: &Known) -> TaskGraph {
        graph_with(conversions, known, &[], &Agents::default())
    }

    fn graph_with(
        conversions: &[serde_yaml::Value],
        known: &Known,
        requests: &[RequestView],
        agents: &Agents,
    ) -> TaskGraph {
        build_task_graph(
            &GraphInput {
                draft_id: "01DRAFT",
                conversions,
                requests,
                now: "2026-11-05T00:00:00Z".into(),
            },
            known,
            agents,
        )
    }

    fn edge<'a>(g: &'a TaskGraph, from: &str, to_prefix: &str) -> &'a GraphEdge {
        g.edges
            .iter()
            .find(|e| {
                e.from == from && e.to.starts_with(to_prefix) && e.kind == EdgeKind::DependsOn
            })
            .unwrap_or_else(|| panic!("no edge {from} -> {to_prefix}*: {:#?}", g.edges))
    }

    // TS-06: tracks, phases, tasks, statuses, current phase and done/total.
    #[test]
    fn a_worktree_shows_its_tracks_phases_tasks_and_progress() {
        let root = temp("ts06");
        let p = plan(vec![
            (
                "phase-1",
                "done",
                vec![task("a", "done", &[]), task("b", "done", &[])],
            ),
            (
                "phase-2",
                "doing",
                vec![task("c", "doing", &[]), task("d", "backlog", &[])],
            ),
        ]);
        let wt = worktree(&root, "wt", &[("t1_20261105", Some(p))]);
        let g = graph(
            &[created("/code/acme", "feat-x", &wt)],
            &Known(vec![wt.clone()]),
        );

        let w = &g.worktrees[0];
        assert_eq!(w.id, "w:0");
        assert_eq!(w.state, WorktreeState::Ok);
        assert_eq!(w.progress, Progress { done: 2, total: 4 });
        let t = &w.tracks[0];
        assert_eq!(t.id, "t:0/t1_20261105");
        assert_eq!(t.current_phase.as_deref(), Some("phase-2"));
        assert_eq!(t.phases[1].tasks[0].id, "k:0/t1_20261105/c");
        assert_eq!(t.phases[1].progress, Progress { done: 0, total: 2 });
        assert_eq!(g.progress, Progress { done: 2, total: 4 });
        assert!(
            g.edges
                .iter()
                .any(|e| e.from == "w:0" && e.to == "t:0/t1_20261105")
        );
    }

    // TS-08: a designed track without a plan is `designing`, 0/0.
    #[test]
    fn a_track_without_a_plan_is_designing() {
        let root = temp("ts08");
        let wt = worktree(&root, "wt", &[("t1_20261105", None)]);
        let g = graph(&[created("/code/acme", "x", &wt)], &Known(vec![wt.clone()]));
        let t = &g.worktrees[0].tracks[0];
        assert!(t.designing);
        assert_eq!(t.progress, Progress::default());
        assert_eq!(t.error_code, None);
    }

    // TS-09 / TS-13: depends_on draws edges; a plan without it draws only
    // containment.
    #[test]
    fn dependencies_draw_edges_and_legacy_plans_draw_none() {
        let root = temp("ts09");
        let p = plan(vec![(
            "phase-1",
            "doing",
            vec![task("a", "done", &[]), task("b", "doing", &["a"])],
        )]);
        let legacy = serde_json::json!({"track_id": "l", "phases": [{"id": "p1", "name": "P", "status": "doing",
            "tasks": [{"id": "x", "name": "X", "status": "doing"}]}]});
        let wt = worktree(
            &root,
            "wt",
            &[("t1_20261105", Some(p)), ("legacy_20261105", Some(legacy))],
        );
        let g = graph(&[created("/code/acme", "x", &wt)], &Known(vec![wt.clone()]));
        let e = edge(&g, "k:0/t1_20261105/b", "k:0/t1_20261105/a");
        assert!(!e.dangling && e.flag.is_none());
        assert!(
            !g.edges
                .iter()
                .any(|e| e.kind == EdgeKind::DependsOn && e.from.contains("legacy"))
        );
    }

    // TS-11 / TS-51: an unknown ref, or a sibling the item does not have, is a
    // flagged dangling edge.
    #[test]
    fn unknown_refs_dangle() {
        let root = temp("ts11");
        let p = plan(vec![(
            "phase-1",
            "doing",
            vec![task(
                "b",
                "doing",
                &["no-such", "@other-branch:t/k", "bad ref"],
            )],
        )]);
        let wt = worktree(&root, "wt", &[("t1_20261105", Some(p))]);
        let g = graph(&[created("/code/acme", "x", &wt)], &Known(vec![wt.clone()]));
        let from = "k:0/t1_20261105/b";
        let deps: Vec<&GraphEdge> = g
            .edges
            .iter()
            .filter(|e| e.from == from && e.kind == EdgeKind::DependsOn)
            .collect();
        assert_eq!(deps.len(), 3);
        assert!(deps.iter().all(|e| e.dangling && e.to.starts_with("x:")));
        assert_eq!(deps[2].flag, Some("malformed"));
        assert_eq!(deps[0].flag, Some("unknown"));
    }

    // TS-36: `@branch:track/task` resolves to the sibling worktree's task.
    #[test]
    fn cross_worktree_refs_resolve_to_the_sibling() {
        let root = temp("ts36");
        let a = worktree(
            &root,
            "a",
            &[(
                "ta_20261105",
                Some(plan(vec![(
                    "phase-1",
                    "doing",
                    vec![task("k1", "doing", &["@feat-b:tb_20261105/k2"])],
                )])),
            )],
        );
        let b = worktree(
            &root,
            "b",
            &[(
                "tb_20261105",
                Some(plan(vec![(
                    "phase-1",
                    "doing",
                    vec![task("k2", "done", &[])],
                )])),
            )],
        );
        let g = graph(
            &[
                created("/code/a", "feat-a", &a),
                created("/code/b", "feat-b", &b),
            ],
            &Known(vec![a.clone(), b.clone()]),
        );
        let e = edge(&g, "k:0/ta_20261105/k1", "k:1/tb_20261105/k2");
        assert!(!e.dangling);
    }

    // TS-54 / TS-63: one branch in two projects gets distinct node ids, and an
    // `@branch:` ref to it is flagged ambiguous.
    #[test]
    fn a_branch_in_two_projects_is_distinct_and_ambiguous() {
        let root = temp("ts54");
        let one = worktree(
            &root,
            "one",
            &[(
                "t_20261105",
                Some(plan(vec![("p", "doing", vec![task("k", "doing", &[])])])),
            )],
        );
        let two = worktree(
            &root,
            "two",
            &[(
                "t_20261105",
                Some(plan(vec![("p", "doing", vec![task("k", "doing", &[])])])),
            )],
        );
        let three = worktree(
            &root,
            "three",
            &[(
                "u_20261105",
                Some(plan(vec![(
                    "p",
                    "doing",
                    vec![task("z", "doing", &["@same:t_20261105/k"])],
                )])),
            )],
        );
        let g = graph(
            &[
                created("/code/p1", "same", &one),
                created("/code/p2", "same", &two),
                created("/code/p3", "other", &three),
            ],
            &Known(vec![one.clone(), two.clone(), three.clone()]),
        );
        assert_eq!(g.worktrees[0].id, "w:0");
        assert_eq!(g.worktrees[1].id, "w:1");
        let e = edge(&g, "k:2/u_20261105/z", "x:");
        assert_eq!(e.flag, Some("ambiguous"));
    }

    // TS-44: a cycle terminates and is flagged.
    #[test]
    fn cycles_are_flagged_not_followed() {
        let root = temp("ts44");
        let p = plan(vec![(
            "p",
            "doing",
            vec![task("a", "doing", &["b"]), task("b", "doing", &["a"])],
        )]);
        let wt = worktree(&root, "wt", &[("t_20261105", Some(p))]);
        let g = graph(&[created("/code/acme", "x", &wt)], &Known(vec![wt.clone()]));
        assert_eq!(
            edge(&g, "k:0/t_20261105/a", "k:0/t_20261105/b").flag,
            Some("cycle")
        );
        assert_eq!(
            edge(&g, "k:0/t_20261105/b", "k:0/t_20261105/a").flag,
            Some("cycle")
        );
    }

    // TS-03 / TS-04 / TS-61 / TS-62 (unit): every way a worktree or track can
    // be unreadable is a state or code, and siblings are unaffected.
    #[test]
    fn unreadable_things_degrade_to_codes() {
        let root = temp("ts61");
        let good = worktree(
            &root,
            "good",
            &[(
                "t_20261105",
                Some(plan(vec![("p", "doing", vec![task("k", "done", &[])])])),
            )],
        );
        let no_ws = root.join("plain");
        std::fs::create_dir_all(&no_ws).unwrap();
        let torn = worktree(&root, "torn", &[("t_20261105", None)]);
        std::fs::create_dir_all(torn.join(".ai/sebenza/tracks/t_20261105")).unwrap();
        std::fs::write(
            torn.join(".ai/sebenza/tracks/t_20261105/plan.json"),
            "{\"phases\": [",
        )
        .unwrap();
        let gone = root.join("gone");
        let g = graph(
            &[
                created("/code/a", "good", &good),
                created("/code/a", "plain", &no_ws),
                created("/code/a", "torn", &torn),
                created("/code/a", "gone", &gone),
                failed("/code/a", "boom"),
            ],
            &Known(vec![good.clone(), no_ws.clone(), torn.clone()]),
        );
        let states: Vec<(WorktreeState, Option<&str>)> = g
            .worktrees
            .iter()
            .map(|w| (w.state, w.error_code))
            .collect();
        assert_eq!(
            states,
            vec![
                (WorktreeState::Ok, None),
                (WorktreeState::NoWorkspace, Some("missing")),
                (WorktreeState::Ok, None),
                (WorktreeState::Unavailable, Some("missing")),
                (WorktreeState::Failed, Some("conversion_failed")),
            ]
        );
        assert_eq!(g.worktrees[2].tracks[0].error_code, Some("invalid_json"));
        assert_eq!(g.failed, 1);
        assert_eq!(g.unavailable, 1);
        assert_eq!(g.progress, Progress { done: 1, total: 1 });
        let json = serde_json::to_string(&g).unwrap();
        assert!(!json.contains("boom at"), "no recorded error text: {json}");
        assert!(
            !json.contains(&root.to_string_lossy().to_string()),
            "no worktree paths"
        );
    }

    // TS-41 / TS-42 / TS-65: confinement. Unregistered dirs, symlinked
    // workspaces and traversal track ids are refused without reading.
    #[cfg(unix)]
    #[test]
    fn confinement_refuses_unregistered_symlinked_and_traversal_paths() {
        let root = temp("ts65");
        let secret = root.join("secret");
        std::fs::create_dir_all(&secret).unwrap();
        std::fs::write(
            secret.join("plan.json"),
            plan(vec![("p", "doing", vec![task("leak", "done", &[])])]).to_string(),
        )
        .unwrap();

        let unregistered = worktree(&root, "unreg", &[("t_20261105", None)]);
        let linked = root.join("linked");
        std::fs::create_dir_all(linked.join(".ai")).unwrap();
        std::os::unix::fs::symlink(unregistered.join(".ai/sebenza"), linked.join(".ai/sebenza"))
            .unwrap();
        let trav = worktree(&root, "trav", &[]);
        std::fs::write(
            trav.join(".ai/sebenza/tracks.json"),
            serde_json::json!({"tracks": [{"track_id": "../../secret"}, {"track_id": "/etc"}]})
                .to_string(),
        )
        .unwrap();
        let g = graph(
            &[
                created("/code/a", "unreg", &unregistered),
                created("/code/a", "linked", &linked),
                created("/code/a", "trav", &trav),
            ],
            &Known(vec![linked.clone(), trav.clone()]),
        );
        assert_eq!(g.worktrees[0].state, WorktreeState::Unavailable);
        assert_eq!(g.worktrees[0].error_code, Some("not_registered"));
        assert_eq!(g.worktrees[1].state, WorktreeState::NoWorkspace);
        assert_eq!(g.worktrees[1].error_code, Some("unreadable"));
        let trav_tracks = &g.worktrees[2].tracks;
        assert!(
            trav_tracks
                .iter()
                .all(|t| t.error_code == Some("invalid_id") && t.phases.is_empty())
        );
        assert!(!serde_json::to_string(&g).unwrap().contains("leak"));
    }

    // TS-43: oversize files and over-cap worktrees are truncated, never fatal.
    #[test]
    fn caps_truncate_instead_of_failing() {
        let root = temp("ts43");
        let big = worktree(&root, "big", &[("t_20261105", None)]);
        let pad = "x".repeat(MAX_FILE_BYTES as usize + 1);
        std::fs::create_dir_all(big.join(".ai/sebenza/tracks/t_20261105")).unwrap();
        std::fs::write(
            big.join(".ai/sebenza/tracks/t_20261105/plan.json"),
            format!("\"{pad}\""),
        )
        .unwrap();
        let mut conversions = vec![created("/code/a", "big", &big)];
        let mut known = vec![big.clone()];
        for i in 0..MAX_WORKTREES {
            let w = worktree(&root, &format!("w{i}"), &[]);
            conversions.push(created("/code/a", &format!("b{i}"), &w));
            known.push(w);
        }
        let g = graph(&conversions, &Known(known));
        assert_eq!(g.worktrees[0].tracks[0].error_code, Some("too_large"));
        assert!(g.truncated);
        assert_eq!(
            g.worktrees.len(),
            MAX_WORKTREES + 1,
            "over-cap worktrees still listed"
        );
        assert_eq!(g.worktrees[MAX_WORKTREES].error_code, Some("over_cap"));
    }

    // TS-75: worktree nodes carry the live agent state, or none.
    #[test]
    fn worktrees_carry_live_agent_state() {
        let root = temp("ts75");
        let a = worktree(&root, "a", &[]);
        let b = worktree(&root, "b", &[]);
        let mut agents = Agents::default();
        agents.0.insert(
            "feat-a".into(),
            AgentState {
                status: "running".into(),
                feedback_state: "waiting".into(),
                agent: Some("claude".into()),
            },
        );
        let g = graph_with(
            &[
                created("/code/a", "feat-a", &a),
                created("/code/a", "feat-b", &b),
            ],
            &Known(vec![a.clone(), b.clone()]),
            &[],
            &agents,
        );
        assert_eq!(
            g.worktrees[0].agent.as_ref().unwrap().feedback_state,
            "waiting"
        );
        assert_eq!(g.worktrees[1].agent, None);
    }

    // TS-78 / TS-77: `current_phase_id` wins over derivation; a plan task's
    // `request` field and an inbox request naming the task both badge it,
    // the inbox one taking precedence.
    #[test]
    fn sibling_written_fields_and_inbox_requests_are_honoured() {
        let root = temp("ts78");
        let mut p = plan(vec![
            ("phase-1", "doing", vec![task("a", "doing", &[])]),
            (
                "phase-2",
                "backlog",
                vec![task("b", "blocked", &[]), task("c", "backlog", &[])],
            ),
        ]);
        p["phases"][1]["tasks"][1]["request"] =
            serde_json::json!({"request_id": null, "status": "unsent", "raised_at": "t"});
        let wt = worktree(&root, "wt", &[("t1_20261105", Some(p))]);
        let mut tracks: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(wt.join(".ai/sebenza/tracks.json")).unwrap(),
        )
        .unwrap();
        tracks["tracks"][0]["current_phase_id"] = serde_json::json!("phase-2");
        std::fs::write(wt.join(".ai/sebenza/tracks.json"), tracks.to_string()).unwrap();

        let request = RequestView {
            request_id: "01REQ".into(),
            worktree: WorktreeKey {
                project: "/code/acme".into(),
                branch: "x".into(),
            },
            title: "t".into(),
            body: "b".into(),
            status: RequestStatus::Open,
            flagged: false,
            proposal_id: None,
            proposal: None,
            proposal_hash: None,
            proposal_warnings: vec![],
            content_hash: None,
            confirmed_text: None,
            last_reason: None,
            last_error: None,
            attempts: 0,
            warnings: vec![],
            opened_at: "t".into(),
            task: Some(TaskLink {
                track_id: "t1_20261105".into(),
                task_id: "b".into(),
            }),
        };
        let g = graph_with(
            &[created("/code/acme", "x", &wt)],
            &Known(vec![wt.clone()]),
            &[request],
            &Agents::default(),
        );
        let t = &g.worktrees[0].tracks[0];
        assert_eq!(t.current_phase.as_deref(), Some("phase-2"));
        let b = &t.phases[1].tasks[0];
        let c = &t.phases[1].tasks[1];
        assert_eq!(
            b.request.as_ref().map(|r| (r.source, r.status.as_str())),
            Some(("inbox", "open"))
        );
        assert_eq!(
            c.request.as_ref().map(|r| (r.source, r.status.as_str())),
            Some(("plan", "unsent"))
        );
    }

    // TS-68 (unit): the etag is stable until a file the graph reads changes.
    #[test]
    fn the_etag_changes_only_when_inputs_do() {
        let root = temp("etag");
        let wt = worktree(
            &root,
            "wt",
            &[(
                "t_20261105",
                Some(plan(vec![("p", "doing", vec![task("k", "doing", &[])])])),
            )],
        );
        let conv = [created("/code/a", "x", &wt)];
        let known = Known(vec![wt.clone()]);
        let input = GraphInput {
            draft_id: "01DRAFT",
            conversions: &conv,
            requests: &[],
            now: "n".into(),
        };
        let first = graph_etag(&input, &known);
        assert_eq!(first, graph_etag(&input, &known));
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(
            wt.join(".ai/sebenza/tracks/t_20261105/plan.json"),
            plan(vec![("p", "done", vec![task("k", "done", &[])])]).to_string(),
        )
        .unwrap();
        assert_ne!(first, graph_etag(&input, &known));
        assert_eq!(
            build_task_graph(&input, &known, &Agents::default()).etag,
            graph_etag(&input, &known)
        );
    }
}
