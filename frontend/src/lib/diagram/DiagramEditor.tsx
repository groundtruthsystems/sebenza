import { useCallback, useEffect, useState, type KeyboardEvent } from "react";
import { ReactFlowProvider } from "@xyflow/react";
import Btn from "../Btn";
import { renderDraftMarkdown } from "../inboxMarkdown";
import { fenceText } from "./fences";
import GraphCanvas, { type ChangeOpts } from "./GraphCanvas";
import SequenceEditor from "./SequenceEditor";
import { placeMissing } from "./layout";
import {
  emptyGraph,
  emptySequence,
  KIND_SPECS,
  SEQUENCE_SPEC,
  type AnyDoc,
  type DiagramKind,
  type GraphKind,
} from "./model";
import { parseDiagram, serializeDiagram } from "./mermaid";

/**
 * The diagram editor: a full-screen overlay over the inbox editor.
 *
 * Its output is always Mermaid source. Canvas edits serialize into it; typing
 * in the Source tab parses back out of it when the canvas can represent the
 * result. When it cannot, the canvas is unavailable and the source is saved
 * exactly as typed - a hand-written diagram is never rewritten unasked.
 */

interface Entry {
  source: string;
  doc: AnyDoc | null;
}

interface History {
  past: Entry[];
  present: Entry;
  future: Entry[];
  lastKey?: string;
  lastAt: number;
}

const COALESCE_MS = 1500;
const MAX_HISTORY = 200;

function fromSource(source: string): Entry {
  const { doc } = parseDiagram(source);
  // Positions for nodes the source did not place; the text stays as typed.
  return { source, doc: doc && doc.kind !== "sequence" ? placeMissing(doc) : doc };
}

function fromDoc(doc: AnyDoc): Entry {
  return { source: serializeDiagram(doc), doc };
}

function commit(h: History, entry: Entry, opts: ChangeOpts = {}): History {
  const now = Date.now();
  if (entry.source === h.present.source && entry.doc === h.present.doc) return h;
  const fold = opts.silent || (opts.coalesce !== undefined && opts.coalesce === h.lastKey && now - h.lastAt < COALESCE_MS);
  if (fold) return { ...h, present: entry, future: [], lastKey: opts.coalesce, lastAt: now };
  return {
    past: [...h.past, h.present].slice(-MAX_HISTORY),
    present: entry,
    future: [],
    lastKey: opts.coalesce,
    lastAt: now,
  };
}

type Tab = "canvas" | "source" | "preview";

const KINDS: { kind: DiagramKind | "source"; title: string; blurb: string }[] = [
  ...(Object.keys(KIND_SPECS) as GraphKind[]).map((k) => ({ kind: k, title: KIND_SPECS[k].title, blurb: KIND_SPECS[k].blurb })),
  { kind: "sequence", title: SEQUENCE_SPEC.title, blurb: SEQUENCE_SPEC.blurb },
  { kind: "source", title: "Mermaid source", blurb: "Type any Mermaid diagram by hand" },
];

export interface DiagramEditorProps {
  /** The fence's source when editing; null to create a new diagram. */
  initialSource: string | null;
  onSave(source: string): void;
  onCancel(): void;
}

export default function DiagramEditor({ initialSource, onSave, onCancel }: DiagramEditorProps) {
  const [history, setHistory] = useState<History | null>(() =>
    initialSource === null ? null : { past: [], present: fromSource(initialSource), future: [], lastAt: 0 },
  );
  const [tab, setTab] = useState<Tab>(() => (history?.present.doc ? "canvas" : "source"));
  const [confirmDiscard, setConfirmDiscard] = useState(false);

  const present = history?.present ?? null;
  const dirty = present !== null && present.source !== (initialSource ?? "");

  const onDocChange = useCallback((doc: AnyDoc, opts?: ChangeOpts) => {
    setHistory((h) => (h ? commit(h, fromDoc(doc), opts) : h));
  }, []);

  const undo = () =>
    setHistory((h) =>
      h && h.past.length
        ? { past: h.past.slice(0, -1), present: h.past[h.past.length - 1], future: [h.present, ...h.future], lastAt: 0 }
        : h,
    );
  const redo = () =>
    setHistory((h) =>
      h && h.future.length
        ? { past: [...h.past, h.present], present: h.future[0], future: h.future.slice(1), lastAt: 0 }
        : h,
    );

  const start = (kind: DiagramKind | "source") => {
    if (kind === "source") {
      setHistory({ past: [], present: { source: "", doc: null }, future: [], lastAt: 0 });
      setTab("source");
      return;
    }
    const doc = kind === "sequence" ? emptySequence() : emptyGraph(kind);
    setHistory({ past: [], present: fromDoc(doc), future: [], lastAt: 0 });
    setTab("canvas");
  };

  const onKeyDown = (e: KeyboardEvent) => {
    const t = e.target as HTMLElement;
    // Inputs keep their own native undo.
    if (t.closest("input, textarea, select")) return;
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key.toLowerCase() === "z") {
      e.preventDefault();
      if (e.shiftKey) redo();
      else undo();
    } else if (mod && e.key.toLowerCase() === "y") {
      e.preventDefault();
      redo();
    }
  };

  const cancel = () => {
    if (dirty && !confirmDiscard) {
      setConfirmDiscard(true);
      return;
    }
    onCancel();
  };

  const kindTitle = (() => {
    const k = present?.doc?.kind ?? parseDiagram(present?.source ?? "").kind;
    if (!k) return "Diagram";
    return k === "sequence" ? SEQUENCE_SPEC.title : KIND_SPECS[k].title;
  })();

  return (
    <div
      className="fixed inset-0 z-50 bg-black/60 flex p-3"
      role="dialog"
      aria-modal="true"
      aria-label="Diagram editor"
      onKeyDown={onKeyDown}
    >
      <div className="flex-1 min-w-0 flex flex-col bg-surface border border-edge rounded-xl overflow-hidden text-primary">
        <header className="flex items-center gap-3 px-4 py-2 border-b border-edge bg-topbar">
          <h2 className="text-sm font-semibold">{kindTitle}</h2>
          {present && (
            <nav className="flex gap-1" role="tablist" aria-label="Editor view">
              {(["canvas", "source", "preview"] as Tab[]).map((t) => (
                <button
                  key={t}
                  role="tab"
                  aria-selected={tab === t}
                  disabled={t === "canvas" && !present.doc}
                  title={t === "canvas" && !present.doc ? "This diagram can only be edited as source" : undefined}
                  className={`px-3 py-1 text-xs rounded-md capitalize cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed ${
                    tab === t ? "bg-active text-accent" : "text-muted hover:bg-hover"
                  }`}
                  onClick={() => setTab(t)}
                >
                  {t}
                </button>
              ))}
            </nav>
          )}
          <div className="flex-1" />
          {present && (
            <>
              <Btn small onClick={undo} disabled={!history?.past.length} title="Undo (Ctrl+Z)">
                Undo
              </Btn>
              <Btn small onClick={redo} disabled={!history?.future.length} title="Redo (Ctrl+Shift+Z)">
                Redo
              </Btn>
            </>
          )}
          {confirmDiscard ? (
            <>
              <span className="text-xs text-warning">Discard your changes?</span>
              <Btn small variant="danger" onClick={onCancel}>
                Discard
              </Btn>
              <Btn small onClick={() => setConfirmDiscard(false)}>
                Keep editing
              </Btn>
            </>
          ) : (
            <Btn small onClick={cancel}>
              Cancel
            </Btn>
          )}
          {present && (
            <Btn small variant="cta" disabled={!present.source.trim()} onClick={() => onSave(present.source)}>
              {initialSource === null ? "Insert diagram" : "Update diagram"}
            </Btn>
          )}
        </header>

        {!present ? (
          <KindPicker onPick={start} />
        ) : tab === "canvas" && present.doc ? (
          present.doc.kind === "sequence" ? (
            <SequenceEditor doc={present.doc} onChange={onDocChange} />
          ) : (
            <ReactFlowProvider key={present.doc.kind}>
              <GraphCanvas doc={present.doc} onChange={onDocChange} />
            </ReactFlowProvider>
          )
        ) : tab === "preview" ? (
          <Preview source={present.source} />
        ) : (
          <SourcePane
            entry={present}
            onChange={(source) => setHistory((h) => (h ? commit(h, fromSource(source), { coalesce: "source" }) : h))}
          />
        )}
      </div>
    </div>
  );
}

function KindPicker({ onPick }: { onPick: (k: DiagramKind | "source") => void }) {
  return (
    <div className="flex-1 overflow-y-auto p-8">
      <h3 className="text-sm font-semibold mb-1">What do you want to draw?</h3>
      <p className="text-xs text-muted mb-5">
        The diagram is saved into the draft as Mermaid, so the agent reads it as text.
      </p>
      <div className="grid gap-3 grid-cols-[repeat(auto-fill,minmax(220px,1fr))] max-w-4xl">
        {KINDS.map((k) => (
          <button
            key={k.kind}
            className="text-left p-4 rounded-lg border border-edge bg-sidebar hover:border-accent cursor-pointer"
            onClick={() => onPick(k.kind)}
          >
            <div className="text-sm font-semibold">{k.title}</div>
            <div className="text-xs text-muted mt-1">{k.blurb}</div>
          </button>
        ))}
      </div>
    </div>
  );
}

function SourcePane({ entry, onChange }: { entry: Entry; onChange: (s: string) => void }) {
  const known = parseDiagram(entry.source).kind;
  return (
    <div className="flex-1 min-h-0 flex flex-col">
      {entry.source.trim() && !entry.doc && (
        <div className="px-4 py-2 text-xs border-b border-edge bg-warning/10 text-warning" role="status">
          {known
            ? "This diagram uses Mermaid features the canvas can't show yet. Edit it here; it will be saved exactly as written."
            : "Not a diagram type the canvas supports. Edit it here; it will be saved exactly as written."}
        </div>
      )}
      <textarea
        aria-label="Mermaid source"
        className="flex-1 min-h-0 p-4 bg-surface text-primary font-mono text-xs resize-none focus:outline-none"
        value={entry.source}
        onChange={(e) => onChange(e.target.value)}
        spellCheck={false}
        placeholder={"classDiagram\n  class Order\n  Order --> Customer"}
      />
    </div>
  );
}

function Preview({ source }: { source: string }) {
  const [html, setHtml] = useState("");
  useEffect(() => {
    let cancelled = false;
    // The same sanitizing renderer the draft preview uses.
    void renderDraftMarkdown(fenceText(source)).then((h) => {
      if (!cancelled) setHtml(h);
    });
    return () => {
      cancelled = true;
    };
  }, [source]);
  return <div className="flex-1 min-h-0 overflow-auto inbox-preview md-body" dangerouslySetInnerHTML={{ __html: html }} />;
}
