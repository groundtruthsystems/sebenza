// One inbox request, or one document of it, full screen (fullscreen_request_20261119).
// It reads the request itself rather than borrowing the inbox's selection, so
// a direct link or a reload shows the same thing as an in-app entry, and an
// unknown id gets the not-found state instead of a blank page.
import { useEffect, useRef, useState } from "react";
import { fetchInboxDraft, type InboxNote } from "./api";
import { useEscLayer } from "./escStack";
import { renderDraftMarkdown } from "./inboxMarkdown";
import { isDefaultOnly } from "./NotesPanel";
import { solutionApi } from "./solutionApi";

/** The parts of a request full screen reads. */
interface FullscreenRequest {
  id: string;
  title: string;
  body: string;
  raw: { text: string; error: string } | null;
  notes?: InboxNote[];
}

interface Section {
  key: string;
  heading: string;
  markdown: string;
}

type Loaded<T> = { kind: "loading" } | { kind: "missing" } | { kind: "ok"; value: T };

const REQUIREMENTS_DOC = "requirements.md";

/** The sections a `doc` key shows; `null` when the request has no such document. */
export function sectionsFor(request: FullscreenRequest, doc: string | null): Section[] | null {
  const brief: Section = { key: "brief", heading: "Brief", markdown: request.raw?.text ?? request.body };
  const notes = request.notes ?? [];
  const noteSections = notes.map((n) => ({ key: n.id, heading: n.title, markdown: n.body }));
  if (doc === null) return isDefaultOnly(notes) ? [brief] : noteSections;
  if (doc === "brief") return [brief];
  const note = noteSections.find((n) => n.key === doc);
  return note ? [note] : null;
}

function useLoaded<T>(load: (() => Promise<T>) | null, deps: unknown[]): Loaded<T> {
  const [state, setState] = useState<Loaded<T>>({ kind: "loading" });
  useEffect(() => {
    if (!load) return;
    let live = true;
    setState({ kind: "loading" });
    load()
      .then((value) => live && setState({ kind: "ok", value }))
      .catch(() => live && setState({ kind: "missing" }));
    return () => {
      live = false;
    };
    // `deps` stands in for `load`, which is a fresh closure every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return state;
}

function Markdown({ source }: { source: string }) {
  const [html, setHtml] = useState("");
  useEffect(() => {
    let live = true;
    void renderDraftMarkdown(source).then((x) => live && setHtml(x));
    return () => {
      live = false;
    };
  }, [source]);
  // Sanitized by renderDraftMarkdown (DOMPurify + strict mermaid).
  return <div className="md-body text-sm" dangerouslySetInnerHTML={{ __html: html }} />;
}

function NotFound({ children }: { children: string }) {
  return (
    <p role="alert" className="m-0 px-4 py-3 text-xs text-danger bg-danger/10 border border-danger rounded-md">
      {children}
    </p>
  );
}

function SectionList({ sections }: { sections: Section[] }) {
  return (
    <>
      {sections.map((s) => (
        <section key={s.key} aria-label={s.heading} className="flex flex-col gap-2">
          <h3 className="m-0 text-sm font-semibold">{s.heading}</h3>
          <Markdown source={s.markdown} />
        </section>
      ))}
    </>
  );
}

function Body({ request, doc, item }: { request: FullscreenRequest; doc: string | null; item: string }) {
  const isRequirements = doc === "requirements";
  const requirements = useLoaded(
    isRequirements ? () => solutionApi.doc(item, REQUIREMENTS_DOC) : null,
    [item, isRequirements],
  );
  if (isRequirements) {
    if (requirements.kind === "loading") return <p className="m-0 text-xs text-muted">Loading…</p>;
    if (requirements.kind === "missing") return <NotFound>This document was not found in the request.</NotFound>;
    return <SectionList sections={[{ key: "requirements", heading: "Requirements", markdown: requirements.value.content }]} />;
  }
  const sections = sectionsFor(request, doc);
  if (!sections) return <NotFound>This document was not found in the request.</NotFound>;
  return <SectionList sections={sections} />;
}

const openerClass =
  "px-2 py-0.5 rounded border border-edge bg-surface cursor-pointer hover:bg-hover focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent";

/** Buttons that open the brief, the requirements (solution items only) and
 *  each note of the open request full screen. */
export function FullscreenOpeners({
  request,
  isSolution,
  onOpen,
}: {
  request: FullscreenRequest;
  isSolution: boolean;
  onOpen: (doc: string) => void;
}) {
  const notes = request.notes ?? [];
  const docs = [
    { key: "brief", label: "brief", text: "Brief" },
    ...(isSolution ? [{ key: "requirements", label: "requirements", text: "Requirements" }] : []),
    ...(isDefaultOnly(notes) ? [] : notes.map((n) => ({ key: n.id, label: `note ${n.title}`, text: n.title }))),
  ];
  return (
    <nav aria-label="Open full screen" className="flex flex-wrap items-center gap-1.5 px-4 py-1 border-b border-edge bg-sidebar text-[11px] text-muted">
      <span>Full screen:</span>
      {docs.map((d) => (
        <button
          key={d.key}
          type="button"
          aria-label={`Open ${d.label} full screen`}
          data-fullscreen-opener={`doc:${d.key}`}
          className={openerClass}
          onClick={() => onOpen(d.key)}
        >
          {d.text}
        </button>
      ))}
    </nav>
  );
}

/** The full-screen view of request `item`, or of its document `doc`. */
export default function RequestFullscreen({
  item,
  doc,
  onClose,
}: {
  item: string;
  doc: string | null;
  onClose: () => void;
}) {
  const request = useLoaded(() => fetchInboxDraft(item) as Promise<FullscreenRequest>, [item]);
  const closeRef = useRef<HTMLButtonElement>(null);
  useEscLayer("fullscreen", true, onClose);
  useEffect(() => closeRef.current?.focus(), []);

  return (
    <div
      role="region"
      aria-label="Full-screen request"
      className="fixed inset-0 z-50 bg-surface text-primary flex flex-col overflow-hidden"
    >
      <div className="bg-topbar border-b border-edge flex items-center gap-3 px-4 min-h-12">
        <h2 className="m-0 text-sm font-semibold truncate">
          {request.kind === "ok" ? request.value.title || "(untitled)" : "Request"}
        </h2>
        <button
          ref={closeRef}
          type="button"
          aria-label="Close full screen"
          className="ml-auto h-8 px-3 rounded-md border border-edge bg-surface text-xs cursor-pointer hover:bg-hover focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent"
          onClick={onClose}
        >
          Close
        </button>
      </div>
      <div className="flex-1 min-h-0 overflow-y-auto p-6 flex flex-col gap-4 max-w-4xl w-full mx-auto">
        {request.kind === "loading" && <p className="m-0 text-xs text-muted">Loading…</p>}
        {request.kind === "missing" && <NotFound>This request was not found, or you cannot open it.</NotFound>}
        {request.kind === "ok" && <Body request={request.value} doc={doc} item={item} />}
      </div>
    </div>
  );
}
