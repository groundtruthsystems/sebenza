import { useEffect, useState } from "react";
import Btn from "./Btn";
import MarkdownEditor from "./MarkdownEditor";
import PhiNotice from "./PhiNotice";
import {
  addInboxNote,
  deleteInboxNote,
  reorderInboxNotes,
  updateInboxNote,
  type InboxNote,
  type NoteWriteOutcome,
} from "./api";

/** Note types offered by name. Any slug is accepted; skills ignore types they
 *  do not know. `acceptance_criteria` is the one agents look for. */
export const NOTE_TYPES: { type: string; label: string }[] = [
  { type: "general", label: "General" },
  { type: "acceptance_criteria", label: "Acceptance Criteria" },
  { type: "context", label: "Context" },
  { type: "constraints", label: "Constraints" },
];

const labelFor = (type: string) => NOTE_TYPES.find((t) => t.type === type)?.label ?? type;

/** True when the item is a single untitled default note — edited as the plain
 *  body, exactly as before notes existed. */
export function isDefaultOnly(notes: InboxNote[]): boolean {
  if (notes.length === 0) return true;
  if (notes.length > 1) return false;
  const [n] = notes;
  return n.id === "n1" && n.type === "general" && n.title === "Note";
}

/** True when the newest conversion that recorded its notes sent a different
 *  set than the item now holds: its worktrees have stale notes. */
export function notesChangedSinceConversion(
  conversions: { outcome: string; notesListHash?: string | null }[],
  listHash: string,
): boolean {
  const sent = [...conversions]
    .reverse()
    .find((c) => c.outcome === "created" && c.notesListHash);
  return !!sent && !!listHash && sent.notesListHash !== listHash;
}

type Changed = (notes: InboxNote[], listHash: string) => void;

/** Add a typed note at the end of the list. */
export function NoteAdder({
  draftId,
  listHash,
  onchange,
  onstale,
  beforeAdd,
}: {
  draftId: string;
  listHash: string;
  onchange: Changed;
  /** The list moved underneath us: the parent should reload it. */
  onstale?: () => void;
  /** Flush anything unsaved before the list changes shape. */
  beforeAdd?: () => Promise<void>;
}) {
  const [type, setType] = useState("acceptance_criteria");
  const [title, setTitle] = useState("Acceptance Criteria");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const add = async () => {
    if (!title.trim()) return;
    setBusy(true);
    setError("");
    try {
      await beforeAdd?.();
      const out = await addInboxNote(draftId, { type, title: title.trim(), body: "" }, listHash);
      if (out.kind === "conflict") {
        setError("The notes changed since you opened them; reloaded — try again.");
        onstale?.();
        return;
      }
      onchange(out.notes, out.listHash);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      className="flex flex-wrap items-center gap-2 px-4 py-2 border-b border-edge bg-sidebar text-xs"
      onSubmit={(e) => {
        e.preventDefault();
        void add();
      }}
    >
      <select
        aria-label="New note type"
        className="h-7 rounded-md border border-edge bg-surface px-2"
        value={type}
        onChange={(e) => {
          const next = e.currentTarget.value;
          // Keep the title in step while it is still the previous default.
          if (!title.trim() || title === labelFor(type)) setTitle(labelFor(next));
          setType(next);
        }}
        disabled={busy}
      >
        {NOTE_TYPES.map((t) => (
          <option key={t.type} value={t.type}>
            {t.label}
          </option>
        ))}
      </select>
      <input
        aria-label="New note title"
        className="h-7 flex-1 min-w-32 rounded-md border border-edge bg-surface px-2"
        value={title}
        maxLength={120}
        onChange={(e) => setTitle(e.currentTarget.value)}
        disabled={busy}
      />
      <Btn type="submit" small variant="accent-outline" disabled={busy || !title.trim()}>
        Add note
      </Btn>
      <PhiNotice className="basis-full" />
      {error && (
        <p className="m-0 basis-full text-danger" role="alert">
          {error}
        </p>
      )}
    </form>
  );
}

function NoteCard({
  draftId,
  note,
  first,
  last,
  busy,
  onsaved,
  onmove,
  ondelete,
  onhelp,
  coordinator = false,
}: {
  draftId: string;
  note: InboxNote;
  first: boolean;
  last: boolean;
  busy: boolean;
  onsaved: Changed;
  onmove: (delta: -1 | 1) => void;
  ondelete: () => void;
  onhelp?: (note: InboxNote) => void;
  /** Last written by the solution coordinator, and unchanged since. */
  coordinator?: boolean;
}) {
  const [title, setTitle] = useState(note.title);
  const [type, setType] = useState(note.type);
  const [body, setBody] = useState(note.body);
  const [conflict, setConflict] = useState<InboxNote | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState("");

  // A new version from the server (after any write) resets the editor.
  useEffect(() => {
    setTitle(note.title);
    setType(note.type);
    setBody(note.body);
    setConflict(null);
  }, [note.hash, note.title, note.type, note.body]);

  const dirty = title !== note.title || type !== note.type || body !== note.body;

  const save = async (againstHash: string) => {
    setSaving(true);
    setError("");
    try {
      const out: NoteWriteOutcome = await updateInboxNote(
        draftId,
        note.id,
        { type, title, body },
        againstHash,
      );
      if (out.kind === "conflict") {
        setConflict(out.current);
        return;
      }
      setConflict(null);
      onsaved(out.notes, out.listHash);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setSaving(false);
    }
  };

  return (
    <section
      aria-label={`Note: ${note.title}`}
      className="border-b border-edge px-4 py-3 flex flex-col gap-2"
    >
      <div className="flex flex-wrap items-center gap-2 text-xs">
        {coordinator && (
          <span
            className="px-1 rounded border border-accent text-accent"
            title="Written by the solution coordinator and not edited since. Edit it to make it yours."
          >
            coordinator
          </span>
        )}
        <select
          aria-label="Note type"
          className="h-7 rounded-md border border-edge bg-surface px-2"
          value={type}
          onChange={(e) => setType(e.currentTarget.value)}
        >
          {[...NOTE_TYPES, ...(NOTE_TYPES.some((t) => t.type === type) ? [] : [{ type, label: type }])].map(
            (t) => (
              <option key={t.type} value={t.type}>
                {t.label}
              </option>
            ),
          )}
        </select>
        <input
          aria-label="Note title"
          className="h-7 flex-1 min-w-32 rounded-md border border-edge bg-surface px-2 font-semibold"
          value={title}
          maxLength={120}
          onChange={(e) => setTitle(e.currentTarget.value)}
        />
        <Btn small disabled={first || busy} onClick={() => onmove(-1)} aria-label="Move note up">
          ↑
        </Btn>
        <Btn small disabled={last || busy} onClick={() => onmove(1)} aria-label="Move note down">
          ↓
        </Btn>
        {onhelp && (
          <Btn small onClick={() => onhelp(note)}>
            Improve with agent
          </Btn>
        )}
        {!confirming ? (
          <Btn small variant="danger-outline" disabled={busy} onClick={() => setConfirming(true)}>
            Delete note
          </Btn>
        ) : (
          <>
            <Btn
              small
              variant="danger-outline"
              disabled={busy}
              onClick={() => {
                setConfirming(false);
                ondelete();
              }}
            >
              Confirm delete
            </Btn>
            <Btn small onClick={() => setConfirming(false)}>
              Cancel
            </Btn>
          </>
        )}
      </div>
      <MarkdownEditor
        value={body}
        onChange={setBody}
        ariaLabel={`Note body: ${note.title}`}
        height="240px"
      />
      {conflict !== null && (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-2 text-xs bg-warning/10 border border-warning rounded px-2 py-1"
        >
          <span className="flex-1">This note changed since you opened it.</span>
          <Btn
            small
            onClick={() => {
              setTitle(conflict.title);
              setType(conflict.type);
              setBody(conflict.body);
              onsaved([], "");
            }}
          >
            Load theirs
          </Btn>
          <Btn small variant="accent-outline" disabled={saving} onClick={() => void save(conflict.hash)}>
            Keep mine
          </Btn>
        </div>
      )}
      <div className="flex items-center gap-2 text-[11px]">
        <Btn
          small
          variant="accent-outline"
          disabled={!dirty || saving || !title.trim()}
          onClick={() => void save(note.hash)}
        >
          Save note
        </Btn>
        {dirty && <span className="text-muted">Unsaved changes</span>}
        {error && <span className="text-danger">{error}</span>}
      </div>
    </section>
  );
}

/**
 * Every note of an inbox item: typed and titled, saved one at a time against
 * that note's hash (so editing one never conflicts with another), reordered
 * and deleted against the list hash. A 409 shows on the note that has it.
 */
export default function NotesPanel({
  draftId,
  notes,
  listHash,
  onchange,
  onhelp,
  onstale,
  coordinatorNotes,
}: {
  draftId: string;
  notes: InboxNote[];
  listHash: string;
  /** The server's list after a write. Empty means "reload it". */
  onchange: Changed;
  onhelp?: (note: InboxNote) => void;
  onstale?: () => void;
  /** note id -> hash of the coordinator's write; the badge shows while it matches. */
  coordinatorNotes?: Record<string, string>;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const structural = async (write: () => Promise<NoteWriteOutcome>) => {
    setBusy(true);
    setError("");
    try {
      const out = await write();
      if (out.kind === "conflict") {
        setError("The notes changed since you opened them; reloaded.");
        onstale?.();
        return;
      }
      onchange(out.notes, out.listHash);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const move = (index: number, delta: -1 | 1) => {
    const order = notes.map((n) => n.id);
    const j = index + delta;
    if (j < 0 || j >= order.length) return;
    [order[index], order[j]] = [order[j], order[index]];
    void structural(() => reorderInboxNotes(draftId, order, listHash));
  };

  return (
    <div className="flex-1 min-h-0 overflow-y-auto">
      <NoteAdder draftId={draftId} listHash={listHash} onchange={onchange} onstale={onstale} />
      {error && (
        <p className="m-0 px-4 py-1 text-xs text-danger" role="status">
          {error}
        </p>
      )}
      {notes.map((n, i) => (
        <NoteCard
          key={n.id}
          draftId={draftId}
          note={n}
          first={i === 0}
          last={i === notes.length - 1}
          busy={busy}
          onsaved={(next, lh) => (next.length ? onchange(next, lh) : onstale?.())}
          onmove={(d) => move(i, d)}
          ondelete={() => void structural(() => deleteInboxNote(draftId, n.id, listHash))}
          onhelp={onhelp}
          coordinator={coordinatorNotes?.[n.id] === n.hash}
        />
      ))}
    </div>
  );
}
