import { useEffect, useState } from "react";
import { Marked, type Tokens } from "marked";
import { fetchConductorFile } from "./api";
import { errorMessage } from "./utils";

// Lazy-load mermaid (large) only when a markdown doc is viewed, so it stays out
// of the main bundle. Initialized once.
let mermaidReady: Promise<typeof import("mermaid").default> | null = null;
function loadMermaid() {
  if (!mermaidReady) {
    mermaidReady = import("mermaid").then((m) => {
      m.default.initialize({ startOnLoad: false, theme: "dark", securityLevel: "loose" });
      return m.default;
    });
  }
  return mermaidReady;
}

// Unique id per rendered diagram (mermaid.render requires a unique DOM id).
let mermaidSeq = 0;

/** Renders a conductor markdown file (spec.md / design.md), rendering ```mermaid
 *  fenced blocks to inline SVG. Mermaid is rendered to SVG *before* the HTML is
 *  set, so the diagram is part of the one-shot `dangerouslySetInnerHTML` and
 *  React never clobbers it on re-render. Content is the user's own local
 *  worktree file (trusted). */
export default function ConductorMarkdown({ branch, path }: { branch: string; path: string }) {
  const [html, setHtml] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError("");

    void (async () => {
      try {
        const res = await fetchConductorFile(branch, path);

        // Collect ```mermaid blocks and leave placeholders; other markdown +
        // code blocks render via marked's default renderer (return false).
        const diagrams: string[] = [];
        const marked = new Marked({
          renderer: {
            code(token: Tokens.Code) {
              if ((token.lang ?? "").trim().toLowerCase() === "mermaid") {
                const idx = diagrams.length;
                diagrams.push(token.text);
                return `<div class="mermaid-slot" data-idx="${idx}"></div>`;
              }
              return false;
            },
          },
        });
        let out = marked.parse(res.content, { async: false }) as string;

        if (diagrams.length > 0) {
          const mermaid = await loadMermaid();
          for (let i = 0; i < diagrams.length; i++) {
            const slot = `<div class="mermaid-slot" data-idx="${i}"></div>`;
            try {
              const { svg } = await mermaid.render(`mmd-${mermaidSeq++}`, diagrams[i]);
              // Function replacement avoids `$` in the SVG being treated specially.
              out = out.replace(slot, () => `<div class="mermaid-diagram">${svg}</div>`);
            } catch {
              out = out.replace(
                slot,
                () => `<pre class="mermaid-error">Failed to render mermaid diagram</pre>`,
              );
            }
          }
        }

        if (!cancelled) setHtml(out);
      } catch (err: unknown) {
        if (!cancelled) setError(errorMessage(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [branch, path]);

  if (loading) return <div className="text-sm text-muted py-8 text-center">Loading…</div>;
  if (error) return <div className="text-sm text-danger py-8 text-center">{error}</div>;
  return <div className="md-body" dangerouslySetInnerHTML={{ __html: html }} />;
}
