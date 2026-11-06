import type { TaskGraph } from "./api";

/** Above this many task nodes (or {@link COLLAPSE_EDGES} dependency edges) in
 *  the whole graph, tracks are drawn as one summary node until expanded. */
export const COLLAPSE_TASKS = 150;
export const COLLAPSE_EDGES = 400;
/** Characters of a name kept in a node label. */
export const LABEL_MAX = 80;

type Worktree = TaskGraph["worktrees"][number];
type Track = Worktree["tracks"][number];

/**
 * A label safe to put inside `["..."]`: one line, at most {@link LABEL_MAX}
 * characters of the original, and with every character that could close the
 * quote or read as markup turned into a mermaid entity. Names come from
 * repo-controlled plans and from operators, so nothing is trusted (T-05).
 */
export function escapeLabel(raw: string): string {
  const flat = raw.replace(/\s+/g, " ").trim();
  const chars = [...flat];
  const kept = chars.slice(0, LABEL_MAX);
  let out = "";
  for (const c of kept) {
    if (c === '"') out += "#quot;";
    else if (c === "<") out += "#lt;";
    else if (c === ">") out += "#gt;";
    else if (c === "#") out += "#35;";
    else if (c === "`") out += "#96;";
    else out += c;
  }
  return chars.length > LABEL_MAX ? `${out}…` : out;
}

const basename = (p: string) => p.split("/").filter(Boolean).pop() ?? p;

const STATE_SUFFIX: Record<Worktree["state"], string> = {
  ok: "",
  unavailable: " · unavailable",
  failed: " · failed",
  no_workspace: " · no workspace",
};

/**
 * The item graph as Mermaid `flowchart` source. Worktrees and tracks are
 * subgraphs, tasks are nodes, `depends_on` edges are arrows (dotted and
 * labelled when they dangle, thick when they are on a cycle). Every internal
 * id is re-keyed to `n<i>`, so nothing from the server is ever mermaid syntax.
 *
 * Large graphs collapse each track to a summary node; `expanded` names the
 * tracks (by graph id) the operator opened anyway.
 */
export function graphToMermaid(
  graph: TaskGraph,
  expanded: ReadonlySet<string> = new Set(),
): { source: string; collapsed: string[] } {
  const keys = new Map<string, string>();
  const key = (id: string) => {
    let k = keys.get(id);
    if (!k) {
      k = `n${keys.size}`;
      keys.set(id, k);
    }
    return k;
  };
  const totalTasks = graph.worktrees.reduce(
    (n, w) => n + w.tracks.reduce((m, t) => m + t.phases.reduce((p, ph) => p + ph.tasks.length, 0), 0),
    0,
  );
  const depEdges = graph.edges.filter((e) => e.kind === "depends_on");
  const big = totalTasks > COLLAPSE_TASKS || depEdges.length > COLLAPSE_EDGES;
  const collapsed: string[] = [];
  /** A task id -> the node it is drawn as (itself, or its collapsed track). */
  const drawnAs = new Map<string, string>();
  const lines = ["flowchart LR"];
  const classes: string[] = [];

  const trackLabel = (t: Track) =>
    escapeLabel(
      `${t.description || t.trackId} (${t.progress.done}/${t.progress.total})` +
        (t.currentPhase ? ` · ${t.currentPhase}` : t.designing ? " · designing" : ""),
    );

  for (const w of graph.worktrees) {
    const wLabel = escapeLabel(
      `${w.branch} · ${basename(w.projectPath)} (${w.progress.done}/${w.progress.total})${STATE_SUFFIX[w.state]}`,
    );
    lines.push(`  subgraph ${key(w.id)}["${wLabel}"]`);
    lines.push("    direction TB");
    if (w.state !== "ok") classes.push(`class ${key(w.id)} ${w.state}`);
    if (w.tracks.length === 0) {
      const empty = key(`${w.id}#empty`);
      lines.push(`    ${empty}["${escapeLabel(w.state === "ok" ? "no tracks yet" : "not readable")}"]`);
    }
    for (const t of w.tracks) {
      const tasks = t.phases.flatMap((p) => p.tasks);
      if (big && tasks.length > 0 && !expanded.has(t.id)) {
        collapsed.push(t.id);
        lines.push(`    ${key(t.id)}[["${trackLabel(t)}"]]`);
        for (const k of tasks) drawnAs.set(k.id, key(t.id));
        continue;
      }
      if (tasks.length === 0) {
        lines.push(`    ${key(t.id)}["${trackLabel(t)}"]`);
        continue;
      }
      lines.push(`    subgraph ${key(t.id)}["${trackLabel(t)}"]`);
      for (const k of tasks) {
        const n = key(k.id);
        drawnAs.set(k.id, n);
        const badge = k.request ? ` · request ${k.request.status}` : "";
        lines.push(`      ${n}["${escapeLabel(`${k.name || k.taskId} · ${k.status}${badge}`)}"]`);
        if (["done", "doing", "blocked"].includes(k.status)) classes.push(`class ${n} ${k.status}`);
      }
      lines.push("    end");
    }
    lines.push("  end");
  }

  const seen = new Set<string>();
  for (const e of depEdges) {
    const from = drawnAs.get(e.from);
    if (!from) continue;
    let to: string;
    if (e.dangling) {
      to = key(e.to);
      if (!seen.has(`node:${to}`)) {
        seen.add(`node:${to}`);
        lines.push(`  ${to}{{"${escapeLabel(`? ${e.to.replace(/^x:/, "")}`)}"}}`);
        classes.push(`class ${to} dangling`);
      }
    } else {
      const target = drawnAs.get(e.to);
      if (!target) continue;
      to = target;
    }
    if (from === to) continue;
    const arrow = e.dangling
      ? `-.->|${e.flag ?? "unknown"}|`
      : e.flag === "cycle"
        ? "==>|cycle|"
        : "-->";
    const line = `  ${from} ${arrow} ${to}`;
    if (!seen.has(line)) {
      seen.add(line);
      lines.push(line);
    }
  }

  lines.push(
    "  classDef done fill:#1f3d2b,stroke:#3fb950",
    "  classDef doing fill:#1f2f4d,stroke:#58a6ff",
    "  classDef blocked fill:#4d1f1f,stroke:#f85149",
    "  classDef failed fill:#4d1f1f,stroke:#f85149",
    "  classDef unavailable fill:#3d3d3d,stroke:#8b949e",
    "  classDef no_workspace fill:#3d3d3d,stroke:#8b949e",
    "  classDef dangling fill:#3d2f00,stroke:#d29922,stroke-dasharray:4",
    ...classes.map((c) => `  ${c}`),
  );
  return { source: lines.join("\n"), collapsed };
}
