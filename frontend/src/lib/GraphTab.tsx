import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Btn from "./Btn";
import { fetchInboxGraph, type TaskGraph } from "./api";
import { graphToMermaid } from "./graphToMermaid";
import { renderMermaidSvg } from "./inboxMarkdown";

/** How often the graph is re-read while the tab is visible. */
export const GRAPH_POLL_MS = 15_000;

type Worktree = TaskGraph["worktrees"][number];

const STATE_LABEL: Record<Worktree["state"], string | null> = {
  ok: null,
  unavailable: "Unavailable",
  failed: "Failed",
  no_workspace: "No workspace",
};

const basename = (p: string) => p.split("/").filter(Boolean).pop() ?? p;

function WorktreeCard({
  w,
  collapsed,
  onexpand,
}: {
  w: Worktree;
  collapsed: string[];
  onexpand: (trackId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const stateLabel = STATE_LABEL[w.state];
  const waiting = w.agent && w.agent.feedbackState !== "none";
  return (
    <section
      aria-label={`Worktree ${w.branch}`}
      className="border border-edge rounded-md p-2 flex flex-col gap-1 text-xs"
    >
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="font-semibold">{w.branch}</span>
        <span className="text-muted">{basename(w.projectPath)}</span>
        {stateLabel && (
          <span
            className={`px-1 rounded border ${w.state === "failed" ? "border-danger text-danger" : "border-edge text-muted"}`}
            title={w.errorCode ?? undefined}
          >
            {stateLabel}
          </span>
        )}
        {w.agent && <span className="px-1 rounded border border-edge text-muted">{w.agent.status}</span>}
        {waiting && <span className="px-1 rounded bg-danger text-white">waiting on you</span>}
        <span className="ml-auto text-muted">
          {w.progress.done}/{w.progress.total} tasks
        </span>
      </div>
      {w.tracks.length > 0 && (
        <button
          type="button"
          className="self-start text-[11px] underline text-muted cursor-pointer"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open}
        >
          {open ? "Hide tracks" : "Show tracks"}
        </button>
      )}
      {open && (
        <ul className="list-none m-0 p-0 flex flex-col gap-2">
          {w.tracks.map((t) => (
            <li key={t.id} className="border-l-2 border-edge pl-2">
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="font-medium">{t.description || t.trackId}</span>
                <span className="text-muted">
                  {t.progress.done}/{t.progress.total}
                  {t.currentPhase ? ` · current phase ${t.currentPhase}` : t.designing ? " · designing" : ""}
                </span>
                {t.errorCode && <span className="text-danger">({t.errorCode})</span>}
                {collapsed.includes(t.id) && (
                  <button
                    type="button"
                    className="underline text-muted cursor-pointer"
                    onClick={() => onexpand(t.id)}
                  >
                    Expand in diagram
                  </button>
                )}
              </div>
              {t.phases.map((p) => (
                <div key={p.id} className="pl-2">
                  <div className="text-muted">
                    {p.name} · {p.status} · {p.progress.done}/{p.progress.total}
                  </div>
                  <ul className="list-none m-0 pl-2">
                    {p.tasks.map((k) => (
                      <li key={k.id} className="flex flex-wrap gap-1.5">
                        <span>{k.name || k.taskId}</span>
                        <span className="text-muted">{k.status}</span>
                        {k.request && (
                          <span className="px-1 rounded border border-warning text-warning">
                            request {k.request.status}
                          </span>
                        )}
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/**
 * The item's worktrees, their tracks and tasks, and the dependency graph.
 *
 * Refresh policy (design decision): fetch when the tab opens and on Refresh,
 * then every {@link GRAPH_POLL_MS} only while the page is visible, with one
 * request in flight. The server's etag makes an unchanged poll a 304, which
 * skips the mermaid render; a failed poll keeps the last graph, marked stale.
 */
export default function GraphTab({ draftId }: { draftId: string }) {
  const [graph, setGraph] = useState<TaskGraph | null>(null);
  const [stale, setStale] = useState(false);
  const [error, setError] = useState("");
  const [svg, setSvg] = useState("");
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const etagRef = useRef<string | undefined>(undefined);
  const inFlight = useRef<AbortController | null>(null);

  const load = useCallback(async () => {
    if (inFlight.current) return;
    const ac = new AbortController();
    inFlight.current = ac;
    try {
      const out = await fetchInboxGraph(draftId, etagRef.current, ac.signal);
      if (ac.signal.aborted) return;
      if (out.kind === "graph") {
        etagRef.current = out.graph.etag;
        setGraph(out.graph);
      }
      setStale(false);
      setError("");
    } catch (err) {
      if (ac.signal.aborted) return;
      setStale(true);
      setError((err as Error).message);
    } finally {
      if (inFlight.current === ac) inFlight.current = null;
    }
  }, [draftId]);

  useEffect(() => {
    etagRef.current = undefined;
    void load();
    const timer = setInterval(() => {
      if (document.visibilityState === "visible") void load();
    }, GRAPH_POLL_MS);
    const onVisibility = () => {
      if (document.visibilityState === "visible") void load();
      else inFlight.current?.abort();
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisibility);
      inFlight.current?.abort();
      inFlight.current = null;
    };
  }, [load]);

  const diagram = useMemo(() => (graph ? graphToMermaid(graph, expanded) : null), [graph, expanded]);

  useEffect(() => {
    if (!diagram) return;
    let alive = true;
    renderMermaidSvg(diagram.source)
      .then((s) => alive && setSvg(s))
      .catch(() => alive && setSvg(""));
    return () => {
      alive = false;
    };
  }, [diagram]);

  return (
    <div className="flex-1 min-h-0 overflow-y-auto p-3 flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2 text-xs">
        {graph && (
          <span>
            {graph.progress.done}/{graph.progress.total} tasks across {graph.worktrees.length} worktrees
            {graph.failed > 0 && ` · ${graph.failed} failed`}
            {graph.unavailable > 0 && ` · ${graph.unavailable} unavailable`}
          </span>
        )}
        {graph && (
          <span className="text-muted">updated {graph.generatedAt.replace("T", " ").slice(0, 19)}</span>
        )}
        <Btn small className="ml-auto" onClick={() => void load()}>
          Refresh
        </Btn>
      </div>
      {stale && (
        <p role="status" className="m-0 text-xs text-warning">
          Showing the last graph — refreshing failed ({error}); it may be stale.
        </p>
      )}
      {graph?.truncated && (
        <p className="m-0 text-xs text-warning">
          This item is larger than the graph shows in full; some worktrees or tasks are summarised.
        </p>
      )}
      {!graph && !stale && <p className="m-0 text-xs text-muted">Loading the graph…</p>}
      {graph && graph.worktrees.length === 0 && (
        <p className="m-0 text-xs text-muted">This item has not been converted into worktrees yet.</p>
      )}
      {graph && (
        <div className="grid gap-2 md:grid-cols-2">
          {graph.worktrees.map((w) => (
            <WorktreeCard
              key={w.id}
              w={w}
              collapsed={diagram?.collapsed ?? []}
              onexpand={(id) => setExpanded((s) => new Set([...s, id]))}
            />
          ))}
        </div>
      )}
      {svg && (
        <div
          aria-label="Dependency graph"
          className="overflow-auto border border-edge rounded-md p-2"
          // Sanitized by renderMermaidSvg (strict mermaid + DOMPurify).
          dangerouslySetInnerHTML={{ __html: svg }}
        />
      )}
    </div>
  );
}
