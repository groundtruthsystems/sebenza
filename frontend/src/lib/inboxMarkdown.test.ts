import { describe, expect, it } from "vitest";
import { renderDraftMarkdown, sanitize } from "./inboxMarkdown";

/**
 * These are security tests, not formatting tests. A draft is free text people
 * paste in from issues, chat and web pages, so everything here is the expected
 * case rather than an exotic one.
 */
describe("inbox markdown sanitizing", () => {
  it("strips an onerror handler from an injected img", async () => {
    const html = await renderDraftMarkdown(
      `notes\n\n<img src=x onerror="alert(1)">\n`,
    );
    expect(html).not.toContain("onerror");
    expect(html).not.toContain("alert(1)");
  });

  it("removes script tags entirely", async () => {
    const html = await renderDraftMarkdown(
      "before\n\n<script>window.stolen = document.cookie</script>\n\nafter",
    );
    expect(html).not.toContain("<script");
    expect(html).not.toContain("document.cookie");
    expect(html).toContain("before");
    expect(html).toContain("after");
  });

  it("drops a javascript: link but keeps an http one", async () => {
    const html = await renderDraftMarkdown(
      "[click me](javascript:alert(1)) and [real](https://example.com)",
    );
    expect(html).not.toContain("javascript:");
    expect(html).toContain("https://example.com");
  });

  it("strips inline event handlers and style", async () => {
    const html = await renderDraftMarkdown(
      `<div onclick="steal()" style="position:fixed;top:0">hi</div>`,
    );
    expect(html).not.toContain("onclick");
    expect(html).not.toContain("steal()");
    expect(html).not.toContain("position:fixed");
  });

  it("removes an iframe", async () => {
    const html = await renderDraftMarkdown(
      `<iframe src="https://evil.test"></iframe>`,
    );
    expect(html).not.toContain("<iframe");
  });

  it("keeps ordinary markdown intact", async () => {
    const html = await renderDraftMarkdown(
      "# Title\n\nSome **bold** text and `code`.\n\n- one\n- two\n",
    );
    expect(html).toContain("<h1");
    expect(html).toContain("<strong>bold</strong>");
    expect(html).toContain("<code>code</code>");
    expect(html).toContain("<li>one</li>");
  });

  // Mermaid needs a real layout engine to measure text, so under happy-dom
  // `mermaid.render` returns an empty SVG rather than a diagram. That makes
  // "does a diagram appear" unprovable here; it is verified in a real browser
  // at the phase checkpoint. What IS provable — and what these assert — is that
  // the block is consumed, the surrounding prose survives, and no placeholder
  // leaks into the output.
  it("consumes a mermaid block and keeps the surrounding prose", async () => {
    const html = await renderDraftMarkdown(
      "before\n\n```mermaid\nflowchart TD\n  a[Start] --> b[End]\n```\n\nafter",
    );
    expect(html).toContain("before");
    expect(html).toContain("after");
    expect(html).not.toContain("data-mermaid-slot");
    expect(html).not.toContain("```mermaid");
  });

  it("never leaves a raw mermaid fence in the output", async () => {
    const html = await renderDraftMarkdown(
      "```mermaid\nflowchart TD\n  a --> b\n```\n\n```mermaid\nflowchart LR\n  c --> d\n```\n",
    );
    expect(html).not.toContain("data-mermaid-slot");
    expect(html).not.toContain("flowchart TD");
    expect(html).not.toContain("flowchart LR");
  });

  it("shows an error for an unparseable diagram instead of breaking the page", async () => {
    const html = await renderDraftMarkdown(
      "```mermaid\nthis is not a diagram at all {{{\n```\n\nstill here",
    );
    expect(html).toContain("still here");
    expect(html).toContain("diagram error");
    expect(html).not.toContain("data-mermaid-slot");
  });

  it("sanitize() is not a pass-through", () => {
    // Guards the sanitizer itself: if DOMPurify were misconfigured or stubbed,
    // every test above could pass vacuously on input that never had a payload.
    const dirty = `<img src=x onerror="alert(1)"><script>bad()</script>`;
    const clean = sanitize(dirty);
    expect(clean).not.toEqual(dirty);
    expect(clean).not.toContain("onerror");
    expect(clean).not.toContain("<script");
  });
});
