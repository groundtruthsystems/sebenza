import { useCallback, useEffect, useState } from "react";
import AgentPicker from "./AgentPicker";
import { renderDraftMarkdown } from "./inboxMarkdown";
import {
  ApiRefusal,
  solutionApi,
  type AgentProfile,
  type FactoryAgent,
  type FactoryConfig,
  type FactoryOverrides,
  type SolutionApi,
  type SolutionView,
} from "./solutionApi";

const POLL_MS = 1500;

function message(e: unknown): string {
  if (e instanceof ApiRefusal) return e.code ? `${e.message} (${e.code})` : e.message;
  return (e as Error).message;
}

/** One rendered document: sanitized markdown with strict mermaid (FT-15). */
function DocView({ html }: { html: string }) {
  return (
    <div
      className="md-body text-sm"
      // Sanitized by renderDraftMarkdown (DOMPurify + strict mermaid).
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

/** The global `factory:` defaults every item inherits (FR-24). */
export function GlobalAgents({ api, agents }: { api: SolutionApi; agents: FactoryAgent[] }) {
  const [config, setConfig] = useState<FactoryConfig | null>(null);
  const [msg, setMsg] = useState("");
  useEffect(() => {
    void api.config().then((c) => setConfig(c.config ?? {})).catch(() => setConfig({}));
  }, [api]);
  if (!config) return null;
  const set = (path: "coordinator" | "designer" | "solution" | "project", p: AgentProfile | null) =>
    setConfig((c) => {
      const next: FactoryConfig = { solution: { ...(c?.solution ?? {}) }, project: { ...(c?.project ?? {}) } };
      const v = p ?? undefined;
      if (path === "coordinator") next.solution!.coordinator = v;
      else if (path === "designer") next.solution!.designer = v;
      else if (path === "solution") next.solution!.reviewer = v;
      else next.project!.reviewer = v;
      return next;
    });
  return (
    <section aria-label="Defaults for every item" className="flex flex-col gap-2 border-t border-edge pt-3">
      <h3 className="m-0 text-sm font-semibold">Defaults for every item</h3>
      <AgentPicker label="Default coordinator" agents={agents} value={config.solution?.coordinator} onChange={(p) => set("coordinator", p)} />
      <AgentPicker label="Default designer" agents={agents} value={config.solution?.designer} onChange={(p) => set("designer", p)} />
      <AgentPicker label="Default solution reviewer" reviewer agents={agents} value={config.solution?.reviewer} onChange={(p) => set("solution", p)} />
      <AgentPicker label="Default project reviewer" reviewer agents={agents} value={config.project?.reviewer} onChange={(p) => set("project", p)} />
      <div className="flex items-center gap-2">
        <button
          type="button"
          className="px-2 py-1 rounded border border-accent"
          onClick={() =>
            void api
              .saveConfig(config)
              .then(() => setMsg("Saved."))
              .catch((e) => setMsg(message(e)))
          }
        >
          Save defaults
        </button>
        {msg && <span role="status">{msg}</span>}
      </div>
    </section>
  );
}

/**
 * The Design tab of a solution item: the review state ("round n of 3"), the
 * documents, escalated problems, the design approval and each project's
 * handover card, and the reviewer settings for this item and globally.
 * Every action is an operator action and goes out with the operator token.
 */
export default function DesignTab({
  draftId,
  api = solutionApi,
  pollMs = POLL_MS,
}: {
  draftId: string;
  api?: SolutionApi;
  pollMs?: number;
}) {
  const [view, setView] = useState<SolutionView | null>(null);
  const [error, setError] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const [doc, setDoc] = useState<{ name: string; content: string; hash: string; html: string } | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [handover, setHandover] = useState<Record<string, { content: string; hash: string }>>({});
  const [agents, setAgents] = useState<FactoryAgent[]>([]);
  const [overrides, setOverrides] = useState<FactoryOverrides>({});
  const [settingsMsg, setSettingsMsg] = useState("");

  const load = useCallback(async () => {
    try {
      setView(await api.view(draftId));
    } catch (e) {
      setError(message(e));
    }
  }, [api, draftId]);

  useEffect(() => {
    void load();
    void api.agents().then((a) => setAgents(a.agents)).catch(() => setAgents([]));
    void api
      .itemAgents(draftId)
      .then((a) => setOverrides(a.overrides ?? {}))
      .catch(() => setOverrides({}));
  }, [api, draftId, load]);

  const running = view?.job?.running ?? false;
  useEffect(() => {
    if (!running) return;
    const t = setInterval(() => void load(), pollMs);
    return () => clearInterval(t);
  }, [running, load, pollMs]);

  const act = async (fn: () => Promise<unknown>) => {
    setError("");
    try {
      await fn();
    } catch (e) {
      setError(message(e));
    }
    await load();
  };

  const openDoc = async (name: string) => {
    setOpen(name);
    setEditing(null);
    try {
      const d = await api.doc(draftId, name);
      setDoc({ ...d, html: await renderDraftMarkdown(d.content) });
    } catch (e) {
      setError(message(e));
    }
  };

  if (!view) {
    return <div className="p-4 text-xs text-muted">{error || "Loading the solution…"}</div>;
  }
  const { state } = view;
  const phase = state.phase;
  const open_problems = state.problems.filter((p) => p.status === "open");
  const designDocs = Object.keys(view.documents).filter((n) => !n.startsWith("handover/"));
  const setRole = (role: "coordinator" | "designer" | "solution" | "project", p: AgentProfile | null) => {
    setOverrides((o) => {
      const next: FactoryOverrides = { ...o, reviewer: { ...(o.reviewer ?? {}) } };
      const v = p ?? undefined;
      if (role === "coordinator") next.coordinator = v;
      else if (role === "designer") next.designer = v;
      else next.reviewer = { ...next.reviewer, [role]: v };
      return next;
    });
  };

  return (
    <div className="flex-1 min-h-0 overflow-y-auto p-4 flex flex-col gap-4 text-xs">
      <section aria-label="Review state" className="flex flex-wrap items-center gap-3">
        <span className="px-1.5 py-0.5 rounded border border-edge">Phase: {phase}</span>
        <span className="px-1.5 py-0.5 rounded border border-accent text-accent">
          Round {state.round} of {view.roundOf}
          {state.cycle > 1 ? ` (cycle ${state.cycle})` : ""}
        </span>
        {running && <span className="text-accent">Working: {view.job.step}…</span>}
        {!running && view.job.errorCode && (
          <span className="text-danger" role="status">
            Last job failed ({view.job.errorCode}): {view.job.error}
          </span>
        )}
        <div className="ml-auto flex gap-2">
          {(phase === "requirements" || (phase === "designing" && !running)) && (
            <button type="button" className="px-2 py-1 rounded border border-edge" onClick={() => void act(() => api.startDesign(draftId))}>
              Start design
            </button>
          )}
          {phase === "revising" && !running && (
            <button type="button" className="px-2 py-1 rounded border border-edge" onClick={() => void act(() => api.review(draftId))}>
              Next review round
            </button>
          )}
          {(phase === "escalated" || phase === "agreed") && !running && (
            <button type="button" className="px-2 py-1 rounded border border-edge" onClick={() => void act(() => api.review(draftId, true))}>
              New review cycle
            </button>
          )}
          {(phase === "agreed" || phase === "escalated" || phase === "approved" || phase === "handover") &&
            !view.designApprovalValid &&
            open_problems.length === 0 && (
              <button type="button" className="px-2 py-1 rounded border border-accent text-accent" onClick={() => void act(() => api.approveDesign(draftId))}>
                Approve design
              </button>
            )}
          {view.designApprovalValid && !running && (
            <button type="button" className="px-2 py-1 rounded border border-edge" onClick={() => void act(() => api.draftHandovers(draftId))}>
              Draft handovers
            </button>
          )}
        </div>
      </section>
      {error && (
        <p role="alert" className="m-0 text-danger">
          {error}
        </p>
      )}
      {view.designApprovalValid ? (
        <p className="m-0 text-success">Design approved as it stands.</p>
      ) : state.designApproval ? (
        <p className="m-0 text-warning">The design changed after approval; the approval is void.</p>
      ) : null}

      <section aria-label="Documents" className="flex flex-col gap-2">
        <h3 className="m-0 text-sm font-semibold">Documents</h3>
        {designDocs.length === 0 && <p className="m-0 text-muted">No documents yet.</p>}
        <div className="flex flex-wrap gap-2">
          {designDocs.map((name) => (
            <button key={name} type="button" aria-pressed={open === name} className="px-2 py-0.5 rounded border border-edge" onClick={() => void openDoc(name)}>
              {name}
            </button>
          ))}
        </div>
        {doc && open === doc.name && (
          <article aria-label={`Document ${doc.name}`} className="border border-edge rounded-md p-3">
            {editing === doc.name ? (
              <>
                <textarea
                  aria-label={`Edit ${doc.name}`}
                  className="w-full min-h-[12rem] rounded-md border border-edge bg-surface p-2"
                  defaultValue={doc.content}
                  onChange={(e) => {
                    const value = e.currentTarget.value;
                    setNotes((n) => ({ ...n, [doc.name]: value }));
                  }}
                />
                <button
                  type="button"
                  className="mt-2 px-2 py-1 rounded border border-accent"
                  onClick={() =>
                    void act(async () => {
                      await api.saveDoc(draftId, doc.name, notes[doc.name] ?? doc.content, doc.hash);
                      await openDoc(doc.name);
                    })
                  }
                >
                  Save
                </button>
              </>
            ) : (
              <>
                <DocView html={doc.html} />
                <button type="button" className="mt-2 px-2 py-1 rounded border border-edge" onClick={() => setEditing(doc.name)}>
                  Edit
                </button>
              </>
            )}
          </article>
        )}
      </section>

      {state.problems.length > 0 && (
        <section aria-label="Escalated problems" className="flex flex-col gap-2">
          <h3 className="m-0 text-sm font-semibold">Escalated problems</h3>
          {state.problems.map((p) => (
            <div key={p.problemId} aria-label={`Problem ${p.problemId}`} className="flex flex-wrap items-center gap-2 border border-edge rounded-md p-2">
              <span className="font-semibold">{p.problemId}</span>
              <span>{p.title}</span>
              <span className={p.status === "open" ? "text-warning" : "text-success"}>{p.status}</span>
              {p.status === "open" && (
                <button
                  type="button"
                  className="ml-auto px-2 py-0.5 rounded border border-edge"
                  onClick={() => void act(() => api.resolve(draftId, p.problemId, "Resolved by the operator's edit to the design."))}
                >
                  Resolve
                </button>
              )}
            </div>
          ))}
        </section>
      )}

      {view.handovers.length > 0 && (
        <section aria-label="Handovers" className="flex flex-col gap-2">
          <h3 className="m-0 text-sm font-semibold">Project handovers</h3>
          {view.handovers.map((h) => {
            const edit = handover[h.slug];
            return (
              <div key={h.slug} aria-label={`Handover ${h.slug}`} className="border border-edge rounded-md p-2 flex flex-col gap-2">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-semibold">{h.slug}</span>
                  <span>{h.status}</span>
                  {h.copyStale && <span className="text-warning">worktree copy is stale</span>}
                  <div className="ml-auto flex gap-2">
                    <button
                      type="button"
                      className="px-2 py-0.5 rounded border border-edge"
                      onClick={() =>
                        void api.doc(draftId, `handover/${h.slug}.md`).then((d) =>
                          setHandover((m) => ({ ...m, [h.slug]: { content: d.content, hash: d.hash } })),
                        )
                      }
                    >
                      Review
                    </button>
                    {h.status !== "converted" && (
                      <>
                        <button
                          type="button"
                          className="px-2 py-0.5 rounded border border-accent text-accent"
                          disabled={!view.designApprovalValid}
                          onClick={() => void act(() => api.approveHandover(draftId, h.slug, h.instructionHash))}
                        >
                          Approve and create worktree
                        </button>
                        <button type="button" className="px-2 py-0.5 rounded border border-edge" onClick={() => void act(() => api.deferHandover(draftId, h.slug))}>
                          Defer
                        </button>
                      </>
                    )}
                  </div>
                </div>
                {edit && (
                  <>
                    <textarea
                      aria-label={`Instruction for ${h.slug}`}
                      className="w-full min-h-[8rem] rounded-md border border-edge bg-surface p-2"
                      value={edit.content}
                      onChange={(e) => {
                        const content = e.currentTarget.value;
                        setHandover((m) => ({ ...m, [h.slug]: { ...edit, content } }));
                      }}
                    />
                    <button
                      type="button"
                      className="self-start px-2 py-0.5 rounded border border-edge"
                      onClick={() => void act(() => api.editHandover(draftId, h.slug, edit.content, edit.hash))}
                    >
                      Save instruction
                    </button>
                  </>
                )}
              </div>
            );
          })}
        </section>
      )}

      <section aria-label="Agents for this item" className="flex flex-col gap-2 border-t border-edge pt-3">
        <h3 className="m-0 text-sm font-semibold">Agents for this item</h3>
        <AgentPicker label="Coordinator" agents={agents} value={overrides.coordinator} onChange={(p) => setRole("coordinator", p)} />
        <AgentPicker label="Designer" agents={agents} value={overrides.designer} onChange={(p) => setRole("designer", p)} />
        <AgentPicker label="Solution reviewer" reviewer agents={agents} value={overrides.reviewer?.solution} onChange={(p) => setRole("solution", p)} />
        <AgentPicker label="Project reviewer" reviewer agents={agents} value={overrides.reviewer?.project} onChange={(p) => setRole("project", p)} />
        <p className="m-0 text-muted">
          Notes, documents and code go to the selected model provider. A reviewer must use a different agent, or the
          same agent with a different, at-least-as-capable model; lightweight models never review.
        </p>
        <div className="flex items-center gap-2">
          <button
            type="button"
            className="px-2 py-1 rounded border border-accent"
            onClick={() =>
              void api
                .saveItemAgents(draftId, overrides)
                .then(() => setSettingsMsg("Saved."))
                .catch((e) => setSettingsMsg(message(e)))
                .then(load)
            }
          >
            Save agents
          </button>
          {settingsMsg && <span role="status">{settingsMsg}</span>}
        </div>
      </section>
      <GlobalAgents api={api} agents={agents} />
    </div>
  );
}
