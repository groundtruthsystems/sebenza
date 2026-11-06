//! The inbox item graph: worktrees, their tracks, phases and tasks, and the
//! dependencies between tasks (`plan.json` `depends_on`).
//!
//! This module holds the pure parts: the dependency reference grammar and the
//! graph types. Reading worktree files is confined and lives in
//! `services::task_graph`.

use serde::Serialize;

/// Done/total task counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

impl Progress {
    pub fn add(&mut self, other: Progress) {
        self.done += other.done;
        self.total += other.total;
    }
}

/// Where a recorded worktree stands as far as the graph can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeState {
    /// Read, with its Sebenza workspace.
    Ok,
    /// Recorded but gone, unreadable, or not a worktree the daemon knows.
    Unavailable,
    /// The conversion of this target failed.
    Failed,
    /// Present, but with no readable `tracks.json`.
    NoWorkspace,
}

/// The live agent of a worktree, from the daemon's in-memory snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentState {
    /// The runtime status (`running`, `idle`, `stopped`, ...).
    pub status: String,
    /// Whether the agent is waiting on a human (`none`, `waiting`, ...).
    pub feedback_state: String,
    pub agent: Option<String>,
}

/// A request attached to a task: from the inbox (the agent named the task)
/// or from the plan's own `request` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRequest {
    pub request_id: Option<String>,
    pub status: String,
    /// `inbox` or `plan`.
    pub source: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskNode {
    pub id: String,
    pub task_id: String,
    pub name: String,
    pub status: String,
    pub depends_on: Vec<String>,
    pub request: Option<TaskRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseNode {
    pub id: String,
    pub phase_id: String,
    pub name: String,
    pub status: String,
    pub progress: Progress,
    pub tasks: Vec<TaskNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackNode {
    pub id: String,
    pub track_id: String,
    pub description: String,
    pub status: String,
    /// The phase in progress (`current_phase_id` when the plan names one).
    pub current_phase: Option<String>,
    /// A design exists but no readable `plan.json` yet.
    pub designing: bool,
    pub error_code: Option<&'static str>,
    pub progress: Progress,
    pub phases: Vec<PhaseNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeNode {
    /// `w:<conversion index>`.
    pub id: String,
    pub branch: String,
    pub project_path: String,
    pub state: WorktreeState,
    /// A fixed code, never a path or recorded error text.
    pub error_code: Option<&'static str>,
    pub progress: Progress,
    pub agent: Option<AgentState>,
    pub tracks: Vec<TrackNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Contains,
    DependsOn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    /// The target is not a node in this graph.
    pub dangling: bool,
    /// Why the edge is flagged: `unknown`, `ambiguous`, `malformed`, `cycle`.
    pub flag: Option<&'static str>,
}

/// The whole graph of one inbox item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskGraph {
    pub draft_id: String,
    pub generated_at: String,
    pub etag: String,
    /// A cap was hit; some worktrees or tasks are summarised or omitted.
    pub truncated: bool,
    /// Done/total over `ok` worktrees.
    pub progress: Progress,
    pub failed: usize,
    pub unavailable: usize,
    pub worktrees: Vec<WorktreeNode>,
    pub edges: Vec<GraphEdge>,
}

/// One `depends_on` entry: `["@" branch ":"] [track_id "/"] task_id`.
///
/// - no `@`: the same worktree; no `/`: the same track;
/// - `@branch:` names a sibling worktree of the same inbox item. Branch names
///   cannot contain `:`, so the first `:` ends the branch;
/// - `track/task` splits at the last `/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepRef {
    pub branch: Option<String>,
    pub track_id: Option<String>,
    pub task_id: String,
}

impl DepRef {
    /// Parse one reference, or `None` when it is malformed (it is then shown
    /// as a dangling edge, never an error).
    pub fn parse(raw: &str) -> Option<DepRef> {
        let raw = raw.trim();
        let (branch, rest) = match raw.strip_prefix('@') {
            Some(scoped) => {
                let (branch, rest) = scoped.split_once(':')?;
                if branch.is_empty() || branch.contains(char::is_whitespace) {
                    return None;
                }
                (Some(branch.to_string()), rest)
            }
            None => (None, raw),
        };
        let (track_id, task_id) = match rest.rsplit_once('/') {
            Some((track, task)) => (Some(track), task),
            None => (None, rest),
        };
        if !is_plan_id(task_id) || track_id.is_some_and(|t| !is_plan_id(t)) {
            return None;
        }
        Some(DepRef {
            branch,
            track_id: track_id.map(str::to_string),
            task_id: task_id.to_string(),
        })
    }
}

/// A track, phase or task id as plans use them: `[A-Za-z0-9_.-]{1,64}`, and
/// never `.` or `..`, so an id joined onto a path cannot climb out of it.
pub fn is_plan_id(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        && s != "."
        && s != ".."
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(branch: Option<&str>, track: Option<&str>, task: &str) -> Option<DepRef> {
        Some(DepRef {
            branch: branch.map(str::to_string),
            track_id: track.map(str::to_string),
            task_id: task.to_string(),
        })
    }

    // TS-35: the contract fixture's refs split by the agreed grammar.
    #[test]
    fn the_fixture_refs_split_by_the_grammar() {
        let plan: serde_json::Value = serde_json::from_str(include_str!(
            "../../testdata/contracts/plan.depends_on.json"
        ))
        .unwrap();
        let refs: Vec<String> = plan["phases"][0]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|t| {
                t["depends_on"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|v| v.as_str().unwrap().to_string())
            })
            .collect();
        let parsed: Vec<Option<DepRef>> = refs.iter().map(|s| DepRef::parse(s)).collect();
        assert_eq!(
            parsed,
            vec![
                r(None, None, "phase-1-task-1"),
                r(None, Some("other_track_20261105"), "phase-2-task-1"),
                r(
                    Some("sebenza-improvements"),
                    Some("inbox_graph_notes_20261105"),
                    "phase-2-task-2"
                ),
                r(None, None, "no-such-task"),
            ]
        );
    }

    #[test]
    fn branches_may_contain_slashes_and_malformed_refs_are_none() {
        assert_eq!(
            DepRef::parse("@feat/x:t/k"),
            r(Some("feat/x"), Some("t"), "k")
        );
        assert_eq!(DepRef::parse("@feat-x:k"), r(Some("feat-x"), None, "k"));
        for bad in [
            "",
            "@:k",
            "@b:",
            "t/",
            "/k",
            "a/../b",
            "has space",
            "@b",
            "x/y/z",
        ] {
            assert_eq!(DepRef::parse(bad), None, "{bad:?}");
        }
    }
}
