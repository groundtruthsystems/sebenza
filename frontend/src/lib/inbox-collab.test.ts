import { describe, expect, it } from "vitest";
import {
  authorLabel,
  draftHelpOutput,
  lineDiff,
  sha1Hex,
  sortInboxItems,
  worktreeLabel,
} from "./inbox-collab";
import type { InboxAgentJob } from "./api-contract";

const item = (id: string, priority: "P0" | "P1" | "P2" | "P3", createdAt: string) => ({
  id,
  priority,
  createdAt,
  isRaw: false,
});

describe("sortInboxItems", () => {
  it("orders by priority, then newest created, with unparseable items last", () => {
    const items = [
      { id: "raw", isRaw: true },
      item("p2-old", "P2", "2026-09-01T00:00:00Z"),
      item("p0", "P0", "2026-08-01T00:00:00Z"),
      item("p2-new", "P2", "2026-09-20T00:00:00Z"),
      item("p3", "P3", "2026-09-30T00:00:00Z"),
    ];
    expect(sortInboxItems(items).map((i) => i.id)).toEqual([
      "p0",
      "p2-new",
      "p2-old",
      "p3",
      "raw",
    ]);
  });

  it("matches the server's tie-break on id and does not mutate its input", () => {
    const items = [
      item("01A", "P1", "2026-09-01T00:00:00Z"),
      item("01B", "P1", "2026-09-01T00:00:00Z"),
    ];
    expect(sortInboxItems(items).map((i) => i.id)).toEqual(["01B", "01A"]);
    expect(items[0].id).toBe("01A");
  });

  it("treats a missing priority as the default P2", () => {
    const items = [
      { id: "none", isRaw: false, createdAt: "2026-09-30T00:00:00Z" },
      item("p1", "P1", "2026-09-01T00:00:00Z"),
      item("p3", "P3", "2026-09-30T00:00:00Z"),
    ];
    expect(sortInboxItems(items).map((i) => i.id)).toEqual(["p1", "none", "p3"]);
  });
});

describe("lineDiff", () => {
  it("marks removed and added lines around unchanged ones", () => {
    expect(lineDiff("a\nb\nc", "a\nB\nc")).toEqual([
      { op: "same", text: "a" },
      { op: "del", text: "b" },
      { op: "add", text: "B" },
      { op: "same", text: "c" },
    ]);
  });

  it("is all-same for identical text", () => {
    expect(lineDiff("x\ny", "x\ny").every((l) => l.op === "same")).toBe(true);
  });

  it("handles a pure append", () => {
    expect(lineDiff("one", "one\ntwo")).toEqual([
      { op: "same", text: "one" },
      { op: "add", text: "two" },
    ]);
  });
});

describe("sha1Hex", () => {
  // The server's content_hash is the SHA-1 hex of the text; an authored
  // resolution must quote the same thing or the confirm is a 409.
  it("matches the server's content hash", async () => {
    expect(await sha1Hex("abc")).toBe("a9993e364706816aba3e25717850c26c9cd0d89d");
    expect(await sha1Hex("")).toBe("da39a3ee5e6b4b0d3255bfef95601890afd80709");
  });
});

describe("labels", () => {
  it("names a worktree by project directory and branch", () => {
    expect(worktreeLabel({ project: "/code/acme", branch: "fix-x" })).toBe(
      "acme · fix-x",
    );
  });

  it("names each author kind", () => {
    expect(authorLabel("operator")).toBe("operator");
    expect(authorLabel("worktree_agent")).toBe("worktree agent");
    expect(authorLabel("system_agent")).toBe("system agent");
  });
});

describe("draftHelpOutput", () => {
  const job = (over: Partial<InboxAgentJob>): InboxAgentJob => ({
    jobId: "J1",
    draftId: "D1",
    kind: "draft_help",
    requestId: null,
    attempt: 1,
    status: "succeeded",
    output: { jobKind: "draft_help", proposed_body: "new", summary: "tidied" },
    error: null,
    reseeded: false,
    enqueuedAt: "",
    startedAt: null,
    finishedAt: null,
    ...over,
  });

  it("reads a succeeded job's proposal", () => {
    expect(draftHelpOutput(job({}))).toEqual({
      jobKind: "draft_help",
      proposed_body: "new",
      summary: "tidied",
    });
  });

  it("is null for an unfinished, failed or malformed job", () => {
    expect(draftHelpOutput(job({ status: "running", output: null }))).toBeNull();
    expect(draftHelpOutput(job({ status: "failed", output: null }))).toBeNull();
    expect(draftHelpOutput(job({ output: { jobKind: "draft_help" } }))).toBeNull();
  });
});
