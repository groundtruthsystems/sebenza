import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  createInboxDraft,
  deleteInboxDraft,
  fetchInboxDraft,
  fetchInboxDrafts,
  loadInboxControlToken,
  patchInboxDraft,
  saveInboxDraftBody,
} from "./api";
import MDEditor from "@uiw/react-md-editor";
import NavRail from "./NavRail";
import NewDraftDialog from "./NewDraftDialog";
import ConfirmDialog from "./ConfirmDialog";
import Btn from "./Btn";
import Toggle from "./Toggle";
import { fetchProjects } from "./api";
import { renderDraftMarkdown } from "./inboxMarkdown";
import { createDebouncer, saveDraftBody, type DraftLike } from "./inbox-editor";

const AUTOSAVE_MS = 800;

type DraftStatus = "Draft" | "Promoted" | "Dropped";

interface ProjectLink {
  path: string;
  name: string | null;
  resolved: boolean;
}

interface Summary {
  id: string;
  title: string;
  status: DraftStatus;
  updatedAt: string;
  project: ProjectLink | null;
  isRaw: boolean;
}

interface Draft extends DraftLike {
  id: string;
  title: string;
  status: DraftStatus;
  project: ProjectLink | null;
  raw: { text: string; error: string } | null;
}

export default function InboxView() {
  const [drafts, setDrafts] = useState<Summary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [body, setBody] = useState("");
  const [preview, setPreview] = useState("");
  const [search, setSearch] = useState("");
  const [includeDropped, setIncludeDropped] = useState(false);
  const [status, setStatus] = useState("");
  const [conflict, setConflict] = useState<{ theirs: string; theirHash: string } | null>(
    null,
  );

  // The rail's project-scoped destinations need a project to point at; with
  // none registered they are hidden rather than linking nowhere.
  const [projectBase, setProjectBase] = useState("");
  const [showNewDialog, setShowNewDialog] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState("");
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState("");

  const loadedRef = useRef<DraftLike>({ body: "", bodyHash: "" });
  const debouncer = useMemo(() => createDebouncer(AUTOSAVE_MS), []);

  const refresh = useCallback(async () => {
    try {
      const data = await fetchInboxDrafts({
        search: search || undefined,
        includeDropped,
      });
      setDrafts(data.drafts);
    } catch (err) {
      setStatus((err as Error).message);
    }
  }, [search, includeDropped]);

  useEffect(() => {
    void loadInboxControlToken().then(refresh);
  }, [refresh]);

  useEffect(() => {
    void fetchProjects()
      .then((projects) => {
        if (projects[0]?.prefix) setProjectBase(`/${projects[0].prefix}`);
      })
      .catch(() => setProjectBase(""));
  }, []);

  useEffect(() => () => debouncer.cancel(), [debouncer]);

  const open = useCallback(async (id: string) => {
    debouncer.cancel();
    setConflict(null);
    setSelectedId(id);
    try {
      const d = (await fetchInboxDraft(id)) as Draft;
      setDraft(d);
      setBody(d.body);
      loadedRef.current = { body: d.body, bodyHash: d.bodyHash };
      setStatus("");
    } catch (err) {
      setStatus((err as Error).message);
    }
  }, [debouncer]);

  // Re-render the preview whenever the body settles.
  useEffect(() => {
    let cancelled = false;
    void renderDraftMarkdown(body).then((html) => {
      if (!cancelled) setPreview(html);
    });
    return () => {
      cancelled = true;
    };
  }, [body]);

  const persist = useCallback(
    async (next: string) => {
      if (!selectedId) return;
      const outcome = await saveDraftBody(
        {
          save: async (expectedHash, b) => {
            const saved = (await saveInboxDraftBody(
              selectedId,
              expectedHash,
              b,
            )) as Draft;
            return { bodyHash: saved.bodyHash };
          },
          reload: async () => (await fetchInboxDraft(selectedId)) as Draft,
        },
        loadedRef.current,
        next,
      );
      switch (outcome.kind) {
        case "saved":
          loadedRef.current = { body: next, bodyHash: outcome.bodyHash };
          setStatus("Saved");
          void refresh();
          break;
        case "unchanged":
          break;
        case "conflict":
          setConflict({ theirs: outcome.theirs, theirHash: outcome.theirHash });
          setStatus("This draft changed on disk.");
          break;
        case "error":
          setStatus(outcome.message);
          break;
      }
    },
    [selectedId, refresh],
  );

  const onBodyChange = (next: string) => {
    setBody(next);
    setStatus("");
    debouncer.schedule(() => void persist(next));
  };

  const takeTheirs = () => {
    if (!conflict) return;
    setBody(conflict.theirs);
    loadedRef.current = { body: conflict.theirs, bodyHash: conflict.theirHash };
    setConflict(null);
    setStatus("Reloaded from disk.");
  };

  const keepMine = async () => {
    if (!conflict || !selectedId) return;
    // Adopt their hash, then write our body over it. Only the body moves —
    // the conversion history in frontmatter is the server's to keep.
    loadedRef.current = { body: conflict.theirs, bodyHash: conflict.theirHash };
    setConflict(null);
    await persist(body);
  };

  const onCreate = async (title: string) => {
    setCreating(true);
    setCreateError("");
    try {
      const d = (await createInboxDraft(title)) as Draft;
      setShowNewDialog(false);
      await refresh();
      await open(d.id);
    } catch (err) {
      // Reported in the dialog, which stays open so the title is not lost.
      setCreateError((err as Error).message);
    } finally {
      setCreating(false);
    }
  };

  const onDelete = async () => {
    if (!draft) return;
    // `confirmed` is what lets the server delete a promoted draft; it refuses
    // otherwise, so the dialog above is the gate, not a formality.
    const promoted = draft.status === "Promoted";
    setDeleting(true);
    setDeleteError("");
    try {
      await deleteInboxDraft(draft.id, promoted);
      setConfirmDelete(false);
      setDraft(null);
      setSelectedId(null);
      setBody("");
      await refresh();
    } catch (err) {
      setDeleteError((err as Error).message);
    } finally {
      setDeleting(false);
    }
  };

  const onDrop = async () => {
    if (!draft) return;
    try {
      await patchInboxDraft(draft.id, { status: "Dropped" });
      await refresh();
      await open(draft.id);
    } catch (err) {
      setStatus((err as Error).message);
    }
  };

  return (
    <div className="flex h-dvh bg-surface text-primary">
      <NavRail active="inbox" projectBase={projectBase} />
      {showNewDialog && (
        <NewDraftDialog
          loading={creating}
          error={createError}
          oncreate={(title) => void onCreate(title)}
          oncancel={() => setShowNewDialog(false)}
        />
      )}
      {confirmDelete && draft && (
        <ConfirmDialog
          message={
            draft.status === "Promoted"
              ? `"${draft.title}" has been converted into worktrees. Deleting it discards the only record of the prompts that were sent. Delete anyway?`
              : `Delete "${draft.title}"?`
          }
          confirmLabel="Delete"
          loading={deleting}
          error={deleteError}
          onconfirm={() => void onDelete()}
          oncancel={() => setConfirmDelete(false)}
        />
      )}

      <aside className="bg-sidebar border-r border-edge flex flex-col overflow-hidden shrink-0 w-[260px]">
        <div className="p-4 border-b border-edge">
          <div className="flex items-center justify-between">
            <h1 className="text-base font-semibold truncate">Inbox</h1>
            <button
              className="h-8 px-2 gap-1.5 rounded-md border border-edge bg-surface text-accent text-xs flex items-center justify-center cursor-pointer hover:bg-hover"
              onClick={() => {
                setCreateError("");
                setShowNewDialog(true);
              }}
              title="New draft"
            >
              <span className="text-lg leading-none">+</span> New
            </button>
          </div>
          <div className="mt-3 flex flex-col gap-2">
            <div className="relative">
              <input
                type="search"
                value={search}
                onChange={(e) => setSearch(e.currentTarget.value)}
                className="w-full h-7 rounded-md border border-edge bg-surface px-2 pr-6 text-xs text-primary placeholder:text-muted focus:outline-none focus:border-accent"
                placeholder="Search drafts"
                aria-label="Search drafts"
              />
            </div>
          </div>
          <div className="mt-2 flex items-center gap-2 text-[11px] text-muted">
            <label className="flex items-center gap-2 cursor-pointer">
              <Toggle
                checked={includeDropped}
                size="sm"
                aria-label="Show dropped drafts"
                onToggle={setIncludeDropped}
              />
              <span>Show dropped</span>
            </label>
          </div>
        </div>

        <ul className="list-none overflow-y-auto flex-1 min-h-0 p-2">
          {drafts.length === 0 && (
            <li className="px-3 py-4 text-xs text-muted text-center">
              No drafts yet.
            </li>
          )}
          {drafts.map((d) => {
            const isActive = d.id === selectedId;
            return (
              <li key={d.id} className="mb-0.5">
                <button
                  type="button"
                  onClick={() => void open(d.id)}
                  className={`w-full px-3 py-2.5 rounded-md border cursor-pointer flex flex-col gap-1 text-left text-sm bg-transparent hover:bg-hover ${
                    isActive ? "bg-active border-accent" : "border-transparent"
                  } ${d.status === "Dropped" ? "opacity-60" : ""}`}
                >
                  <span className="font-medium truncate">
                    {d.isRaw ? "(unparseable)" : d.title || "(untitled)"}
                  </span>
                  <span className="flex min-w-0 flex-wrap items-center gap-1.5 text-[10px] text-muted">
                    {d.status !== "Draft" && (
                      <span className="shrink-0 px-1.5 py-0.5 rounded border border-edge">
                        {d.status}
                      </span>
                    )}
                    {d.project && (
                      <span
                        className={`shrink-0 px-1.5 py-0.5 rounded border ${
                          d.project.resolved
                            ? "border-edge"
                            : "border-danger text-danger"
                        }`}
                      >
                        {d.project.resolved ? d.project.name : "unresolved project"}
                      </span>
                    )}
                    {d.isRaw && (
                      <span className="shrink-0 text-danger">does not parse</span>
                    )}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </aside>

      <main className="flex-1 min-w-0 flex flex-col overflow-hidden">
        <div className="bg-topbar border-b border-edge flex items-center justify-between gap-3 px-4 min-h-12">
          <div className="flex items-center gap-2 min-w-0">
            <h2 className="text-sm font-semibold truncate">
              {draft ? draft.title : "Inbox"}
            </h2>
            {draft && draft.status !== "Draft" && (
              <span className="shrink-0 text-[10px] px-1.5 py-0.5 rounded border border-edge text-muted">
                {draft.status}
              </span>
            )}
          </div>
          {draft && !draft.raw && (
            <div className="flex items-center gap-2 shrink-0">
              {status && <span className="text-[11px] text-muted">{status}</span>}
              <Btn onClick={() => void onDrop()}>Drop</Btn>
              <Btn
                variant="danger-outline"
                onClick={() => {
                  setDeleteError("");
                  setConfirmDelete(true);
                }}
              >
                Delete
              </Btn>
            </div>
          )}
        </div>

        {!draft && status && (
          <p
            className="px-4 py-3 text-xs text-danger bg-danger/10 border-b border-danger"
            role="alert"
          >
            {status}
          </p>
        )}

        {!draft && (
          <div className="flex-1 flex items-center justify-center text-xs text-muted">
            Select a draft, or create one.
          </div>
        )}

        {draft?.raw && (
          <div className="p-4 overflow-auto">
            <p className="text-xs text-danger mb-2">
              This draft does not parse: {draft.raw.error}
            </p>
            <pre className="text-[11px] text-muted whitespace-pre-wrap font-mono">
              {draft.raw.text}
            </pre>
          </div>
        )}

        {draft && !draft.raw && (
          <>
            {conflict && (
              <div
                className="flex items-center gap-3 px-4 py-2 text-xs bg-warning/10 border-b border-warning"
                role="alert"
              >
                <p className="flex-1 m-0">
                  This draft changed on disk while you were editing. Keep your
                  version, or load theirs?
                </p>
                <Btn small onClick={takeTheirs}>
                  Load theirs
                </Btn>
                <Btn small variant="accent-outline" onClick={() => void keepMine()}>
                  Keep mine
                </Btn>
              </div>
            )}
            <div className="flex-1 min-h-0 flex" data-color-mode="dark">
              <MDEditor
                value={body}
                onChange={(next) => onBodyChange(next ?? "")}
                height="100%"
                visibleDragbar={false}
                textareaProps={{
                  "aria-label": "Draft body",
                  spellCheck: false,
                }}
                components={{
                  // MDEditor's own preview renders markdown its own way, which
                  // would bypass DOMPurify and mermaid's strict mode. Drafts are
                  // pasted-in text, so the preview stays ours.
                  preview: () => (
                    <div
                      className="inbox-preview md-body"
                      dangerouslySetInnerHTML={{ __html: preview }}
                    />
                  ),
                }}
              />
            </div>
          </>
        )}
      </main>
    </div>
  );
}
