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

  const onCreate = async () => {
    const title = window.prompt("Draft title");
    if (!title) return;
    try {
      const d = (await createInboxDraft(title)) as Draft;
      await refresh();
      await open(d.id);
    } catch (err) {
      setStatus((err as Error).message);
    }
  };

  const onDelete = async () => {
    if (!draft) return;
    const promoted = draft.status === "Promoted";
    const ok = window.confirm(
      promoted
        ? "This draft has been converted into worktrees. Deleting it discards the only record of the prompts that were sent. Delete anyway?"
        : `Delete "${draft.title}"?`,
    );
    if (!ok) return;
    try {
      await deleteInboxDraft(draft.id, promoted);
      setDraft(null);
      setSelectedId(null);
      setBody("");
      await refresh();
    } catch (err) {
      setStatus((err as Error).message);
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
    <div className="inbox" data-testid="inbox">
      <aside className="inbox-list">
        <div className="inbox-list-head">
          <input
            aria-label="Search drafts"
            placeholder="Search drafts"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <button type="button" onClick={onCreate}>
            New
          </button>
        </div>
        <label className="inbox-filter">
          <input
            type="checkbox"
            checked={includeDropped}
            onChange={(e) => setIncludeDropped(e.target.checked)}
          />
          Show dropped
        </label>
        <ul>
          {drafts.map((d) => (
            <li key={d.id}>
              <button
                type="button"
                className={d.id === selectedId ? "selected" : ""}
                onClick={() => void open(d.id)}
              >
                <span className="inbox-title">
                  {d.isRaw ? "(unparseable)" : d.title || "(untitled)"}
                </span>
                {d.status !== "Draft" && (
                  <span className="inbox-badge">{d.status}</span>
                )}
                {d.project && (
                  <span
                    className={
                      d.project.resolved ? "inbox-project" : "inbox-project unresolved"
                    }
                  >
                    {d.project.resolved ? d.project.name : "unresolved"}
                  </span>
                )}
              </button>
            </li>
          ))}
          {drafts.length === 0 && <li className="inbox-empty">No drafts yet.</li>}
        </ul>
      </aside>

      <section className="inbox-editor">
        {!draft && <p className="inbox-empty">Select a draft, or create one.</p>}
        {draft?.raw && (
          <div className="inbox-raw">
            <p>This draft does not parse: {draft.raw.error}</p>
            <pre>{draft.raw.text}</pre>
          </div>
        )}
        {draft && !draft.raw && (
          <>
            <header className="inbox-editor-head">
              <h2>{draft.title}</h2>
              <div className="inbox-actions">
                <span className="inbox-status">{status}</span>
                <button type="button" onClick={() => void onDrop()}>
                  Drop
                </button>
                <button type="button" onClick={() => void onDelete()}>
                  Delete
                </button>
              </div>
            </header>

            {conflict && (
              <div className="inbox-conflict" role="alert">
                <p>
                  This draft changed on disk while you were editing. Keep your
                  version, or load theirs?
                </p>
                <button type="button" onClick={takeTheirs}>
                  Load theirs
                </button>
                <button type="button" onClick={() => void keepMine()}>
                  Keep mine
                </button>
              </div>
            )}

            <div className="inbox-split">
              <textarea
                aria-label="Draft body"
                value={body}
                onChange={(e) => onBodyChange(e.target.value)}
                spellCheck={false}
              />
              <div
                className="inbox-preview md-body"
                // Sanitized in renderDraftMarkdown: DOMPurify plus mermaid at
                // securityLevel "strict". Drafts are pasted-in text, so this is
                // deliberately not TrackMarkdown's trusted-file path.
                dangerouslySetInnerHTML={{ __html: preview }}
              />
            </div>
          </>
        )}
      </section>
    </div>
  );
}
