import { Marked } from "marked";
import DOMPurify from "dompurify";

/**
 * Markdown + mermaid rendering for inbox drafts.
 *
 * Deliberately *not* `TrackMarkdown`. That component renders the user's own
 * worktree files and says so — it feeds raw `marked` output to
 * `dangerouslySetInnerHTML` and runs mermaid at `securityLevel: "loose"`.
 * Both are reasonable for a file you wrote yourself and fatal for a draft,
 * which is free text people paste into from issues, chat and web pages.
 *
 * So inbox content goes through DOMPurify, and mermaid runs at
 * `securityLevel: "strict"` — which costs the `click` directive, a trade worth
 * making since a `click` in a pasted diagram is script execution.
 */

// Lazy-load mermaid (large) so it stays out of the main bundle. Initialized
// once, strictly.
let mermaidReady: Promise<typeof import("mermaid").default> | null = null;
function loadMermaid() {
  if (!mermaidReady) {
    mermaidReady = import("mermaid").then((m) => {
      m.default.initialize({
        startOnLoad: false,
        theme: "dark",
        // strict: no raw HTML in diagrams, no click handlers.
        securityLevel: "strict",
        // Render node labels as SVG <text> rather than HTML inside a
        // <foreignObject>. The sanitizer strips foreignObject - it is a way to
        // smuggle arbitrary HTML through an SVG - which would otherwise leave
        // every node an empty box.
        flowchart: { htmlLabels: false },
        htmlLabels: false,
      });
      return m.default;
    });
  }
  return mermaidReady;
}

let mermaidSeq = 0;

const PURIFY_CONFIG = {
  USE_PROFILES: { html: true, svg: true, svgFilters: true },
  // Block every URL scheme that can execute, notably javascript:.
  ALLOWED_URI_REGEXP: /^(?:https?|mailto|tel|ftp):|^[#/]/i,
  ADD_ATTR: ["target"],
  FORBID_TAGS: ["style", "form", "input", "button", "iframe", "object", "embed"],
  FORBID_ATTR: ["style", "onerror", "onload", "onclick"],
};

/** How many times `sanitize` will re-run before giving up on a fixed point. */
const MAX_PASSES = 5;

/**
 * Sanitize markdown output and mermaid's SVG.
 *
 * Runs DOMPurify to a **fixed point** — repeatedly until the output stops
 * changing — rather than once. In a browser that is a no-op: the first pass is
 * already idempotent, so the second returns an identical string and the loop
 * exits after two cheap calls.
 *
 * It matters under happy-dom, where `NodeIterator` does not implement the
 * pre-removing steps correctly: removing a node makes the walk skip its next
 * sibling, so `<script>a()</script><script>b()</script>` sanitizes to
 * `<script>b()</script>` in one pass. That is a test-environment defect, not a
 * DOMPurify one, but iterating to a fixed point costs nothing and means the
 * suite verifies the property we actually care about instead of encoding the
 * bug. See `inboxMarkdown.test.ts`.
 */
export function sanitize(html: string): string {
  let current = html;
  for (let i = 0; i < MAX_PASSES; i++) {
    const next = DOMPurify.sanitize(current, PURIFY_CONFIG) as unknown as string;
    if (next === current) return next;
    current = next;
  }
  return current;
}

/**
 * Sanitize one mermaid SVG, in a single pass.
 *
 * Deliberately not the fixed-point loop above: re-parsing a large SVG as HTML
 * repeatedly compounds happy-dom's iterator bug and can consume the whole diagram.
 * One pass is the right amount here because the input is not arbitrary — it is
 * mermaid's own output, generated at `securityLevel: "strict"`, so this is
 * defence in depth rather than the primary control.
 */
function sanitizeSvg(svg: string): string {
  return DOMPurify.sanitize(svg, {
    USE_PROFILES: { svg: true, svgFilters: true },
    ADD_TAGS: ["style"],
    FORBID_TAGS: ["script", "foreignObject", "iframe"],
    FORBID_ATTR: ["onload", "onerror", "onclick"],
  }) as unknown as string;
}

/**
 * Render draft markdown to sanitized HTML, turning ```mermaid fences into
 * inline SVG first so the diagram survives the single `innerHTML` write.
 *
 * A diagram that fails to parse becomes a visible error block rather than
 * breaking the page — a half-typed diagram is the normal state of a draft.
 */
export async function renderDraftMarkdown(source: string): Promise<string> {
  const diagrams = new Map<string, string>();
  const marked = new Marked({
    async: false,
    gfm: true,
    breaks: false,
  });

  // Collect mermaid blocks, leaving a placeholder that survives sanitizing.
  const withPlaceholders = source.replace(
    /```mermaid\n([\s\S]*?)```/g,
    (_match, code: string) => {
      const key = `sebenza-mermaid-${mermaidSeq++}`;
      diagrams.set(key, code);
      return `\n<div data-mermaid-slot="${key}"></div>\n`;
    },
  );

  // Sanitize the prose first, while the diagrams are still inert placeholders.
  // Keeping SVG out of this pass is what lets it iterate safely.
  let html = sanitize(taskBoxes(marked.parse(withPlaceholders) as string));

  if (diagrams.size > 0) {
    const mermaid = await loadMermaid();
    for (const [key, code] of diagrams) {
      let replacement: string;
      try {
        const { svg } = await mermaid.render(`${key}-svg`, code);
        const clean = sanitizeSvg(svg);
        // Mermaid needs a real layout engine to measure text, so in a
        // headless DOM it can return an empty string rather than throwing.
        // Say so instead of silently dropping the block.
        replacement = clean.trim()
          ? clean
          : `<pre class="mermaid-error">diagram could not be rendered here</pre>`;
      } catch (err) {
        // A half-typed diagram is the normal state of a draft, so this is a
        // visible note rather than a failure.
        const message = err instanceof Error ? err.message : String(err);
        replacement = `<pre class="mermaid-error">diagram error: ${escapeHtml(
          message,
        )}</pre>`;
      }
      html = html.replace(`<div data-mermaid-slot="${key}"></div>`, replacement);
    }
  }

  return html;
}

/**
 * Turn GFM task-list checkboxes into glyphs.
 *
 * DOMPurify strips `type` from `<input>` as a hardening measure, which would
 * leave every task item as a stray text box. A glyph is also the more honest
 * rendering: this is a preview, so a checkbox you cannot tick would only
 * invite clicking. Editing happens in the markdown pane.
 */
function taskBoxes(html: string): string {
  return html.replace(
    /<input([^>]*?)type="checkbox"([^>]*?)>/g,
    (match) =>
      /checked/.test(match)
        ? '<span class="task-box task-done">\u2611</span>'
        : '<span class="task-box">\u2610</span>',
  );
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
