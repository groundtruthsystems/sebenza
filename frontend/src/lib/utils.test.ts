import { beforeEach, describe, expect, it } from "vitest";
import {
  LAST_SELECTED_WORKTREE_STORAGE_KEY,
  WEB_CHAT_UI_STORAGE_KEY,
  loadSavedSelectedWorktree,
  loadUseWebChatUi,
  resolveSelectedBranch,
  saveSelectedWorktree,
  saveUseWebChatUi,
} from "./utils";

describe("worktree selection persistence", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("keeps the saved branch before the first successful worktree load", () => {
    expect(resolveSelectedBranch("feature/last-used", undefined, [], false)).toBe("feature/last-used");
  });

  it("keeps the current selection when that worktree still exists", () => {
    expect(
      resolveSelectedBranch(
        "feature/last-used",
        { branch: "feature/last-used" },
        [{ branch: "feature/last-used", mux: "✗", kind: "linked" }],
        true,
      ),
    ).toBe("feature/last-used");
  });

  it("prefers an open worktree over the repo row", () => {
    // A main session left open should not become the default landing selection
    // ahead of actual work.
    expect(
      resolveSelectedBranch(
        null,
        undefined,
        [
          { branch: "main", mux: "✓", kind: "main" },
          { branch: "feature/x", mux: "✓", kind: "linked" },
        ],
        true,
      ),
    ).toBe("feature/x");
  });

  it("prefers a closed worktree over an open repo row", () => {
    expect(
      resolveSelectedBranch(
        null,
        undefined,
        [
          { branch: "main", mux: "✓", kind: "main" },
          { branch: "feature/x", mux: "✗", kind: "linked" },
        ],
        true,
      ),
    ).toBe("feature/x");
  });

  it("falls back to the repo row when it is the only entry", () => {
    expect(
      resolveSelectedBranch(null, undefined, [{ branch: "main", mux: "✓", kind: "main" }], true),
    ).toBe("main");
  });

  it("falls back to an open worktree when the saved branch is gone", () => {
    expect(
      resolveSelectedBranch(
        "feature/missing",
        undefined,
        [
          { branch: "feature/first", mux: "✗", kind: "linked" },
          { branch: "feature/open", mux: "✓", kind: "linked" },
        ],
        true,
      ),
    ).toBe("feature/open");
  });

  it("stores and clears the last selected worktree", () => {
    saveSelectedWorktree("feature/last-used");

    expect(loadSavedSelectedWorktree()).toBe("feature/last-used");
    expect(localStorage.getItem(LAST_SELECTED_WORKTREE_STORAGE_KEY)).toBe("feature/last-used");

    saveSelectedWorktree(null);

    expect(loadSavedSelectedWorktree()).toBeNull();
    expect(localStorage.getItem(LAST_SELECTED_WORKTREE_STORAGE_KEY)).toBeNull();
  });

  it("stores and clears the web chat UI preference", () => {
    expect(loadUseWebChatUi()).toBe(false);

    saveUseWebChatUi(true);

    expect(loadUseWebChatUi()).toBe(true);
    expect(localStorage.getItem(WEB_CHAT_UI_STORAGE_KEY)).toBe("true");

    saveUseWebChatUi(false);

    expect(loadUseWebChatUi()).toBe(false);
    expect(localStorage.getItem(WEB_CHAT_UI_STORAGE_KEY)).toBeNull();
  });
});
