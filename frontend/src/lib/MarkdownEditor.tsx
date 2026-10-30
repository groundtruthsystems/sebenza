import { lazy, Suspense, useEffect, useState, type MouseEvent } from "react";
import MDEditor, { commands, type ICommand } from "@uiw/react-md-editor";
import { renderDraftMarkdown } from "./inboxMarkdown";
import { fenceAt, insertFence, listFences, replaceFence } from "./diagram/fences";

// The canvas and its layout engine are large and only needed once someone
// opens a diagram, so they load on demand.
const DiagramEditor = lazy(() => import("./diagram/DiagramEditor"));

/** Which diagram the editor is open on: an existing fence, or a new one at `insertAt`. */
type DiagramTarget = { index: number; source: string } | { index: null; insertAt: number };

/**
 * The markdown editor shared by inbox drafts and track docs: source on the
 * left, our own sanitized preview on the right, and a toolbar command plus
 * preview buttons that open the diagram editor on a ```mermaid fence.
 *
 * Saving is the caller's business — this only reports each change.
 */
export default function MarkdownEditor({
  value,
  onChange,
  ariaLabel,
  height = "100%",
  onNotice,
}: {
  value: string;
  onChange: (next: string) => void;
  ariaLabel: string;
  height?: string | number;
  /** Something the caller should surface in its status line. */
  onNotice?: (message: string) => void;
}) {
  const [preview, setPreview] = useState("");
  const [diagram, setDiagram] = useState<DiagramTarget | null>(null);

  // Re-render the preview whenever the body settles.
  useEffect(() => {
    let cancelled = false;
    void renderDraftMarkdown(value, { editableDiagrams: true }).then((html) => {
      if (!cancelled) setPreview(html);
    });
    return () => {
      cancelled = true;
    };
  }, [value]);

  /**
   * Open the diagram editor from the toolbar: on the fence under the cursor
   * when there is one, otherwise to insert a new diagram at the cursor.
   */
  const openDiagramAt = (offset: number) => {
    const fence = fenceAt(value, offset);
    setDiagram(fence ? { index: fence.index, source: fence.source } : { index: null, insertAt: offset });
  };

  const diagramCommand: ICommand = {
    name: "diagram",
    keyCommand: "diagram",
    buttonProps: { "aria-label": "Insert or edit a diagram", title: "Diagram (UML, flowchart, sequence…)" },
    icon: (
      <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden>
        <rect x="1" y="1" width="6" height="4" />
        <rect x="9" y="11" width="6" height="4" />
        <path d="M4 5v4h8v2" />
      </svg>
    ),
    execute: (state) => openDiagramAt(state.selection.start),
  };

  /** The preview's "Edit diagram" buttons, found by delegation. */
  const onPreviewClick = (e: MouseEvent) => {
    const button = (e.target as HTMLElement).closest<HTMLElement>("[data-diagram-edit]");
    if (!button) return;
    const index = Number(button.dataset.diagramEdit);
    const fence = listFences(value)[index];
    if (fence) setDiagram({ index, source: fence.source });
  };

  const saveDiagram = (source: string) => {
    if (!diagram) return;
    const replaced = diagram.index === null ? null : replaceFence(value, diagram.index, source);
    // A fence that went away underneath (a merge or a reload) keeps the work
    // by appending it rather than dropping it.
    const next =
      replaced ?? insertFence(value, diagram.index === null ? diagram.insertAt : value.length, source);
    setDiagram(null);
    onChange(next);
    if (diagram.index !== null && replaced === null) {
      onNotice?.("The original diagram had changed, so this one was added at the end.");
    }
  };

  return (
    <>
      <div className="flex-1 min-w-0 min-h-0 flex" data-color-mode="dark">
        <MDEditor
          value={value}
          onChange={(next) => onChange(next ?? "")}
          commands={[...commands.getCommands(), commands.divider, diagramCommand]}
          height={height}
          visibleDragbar={false}
          textareaProps={{
            "aria-label": ariaLabel,
            spellCheck: false,
          }}
          components={{
            // MDEditor's own preview renders markdown its own way, which
            // would bypass DOMPurify and mermaid's strict mode. Drafts are
            // pasted-in text, so the preview stays ours.
            preview: () => (
              <div
                className="inbox-preview md-body"
                onClick={onPreviewClick}
                dangerouslySetInnerHTML={{ __html: preview }}
              />
            ),
          }}
        />
      </div>
      {diagram && (
        <Suspense
          fallback={
            <div className="fixed inset-0 z-50 bg-black/60 flex items-center justify-center text-xs text-muted">
              Loading diagram editor…
            </div>
          }
        >
          <DiagramEditor
            initialSource={diagram.index === null ? null : diagram.source}
            onSave={saveDiagram}
            onCancel={() => setDiagram(null)}
          />
        </Suspense>
      )}
    </>
  );
}
