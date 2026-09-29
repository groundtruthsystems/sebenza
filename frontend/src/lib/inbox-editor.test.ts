import { describe, expect, it, vi } from "vitest";
import { createDebouncer, isConflict, saveDraftBody } from "./inbox-editor";

const loaded = { body: "original", bodyHash: "hash-1" };

describe("saveDraftBody", () => {
  it("does nothing when the body has not changed", async () => {
    const save = vi.fn();
    const out = await saveDraftBody(
      { save, reload: vi.fn() },
      loaded,
      "original",
    );
    expect(out).toEqual({ kind: "unchanged" });
    expect(save).not.toHaveBeenCalled();
  });

  it("saves against the hash it was loaded at", async () => {
    const save = vi.fn().mockResolvedValue({ bodyHash: "hash-2" });
    const out = await saveDraftBody({ save, reload: vi.fn() }, loaded, "edited");
    expect(save).toHaveBeenCalledWith("hash-1", "edited");
    expect(out).toEqual({ kind: "saved", bodyHash: "hash-2" });
  });

  it("reports a conflict with the other writer's text instead of overwriting", async () => {
    const save = vi.fn().mockRejectedValue({ status: 409 });
    const reload = vi
      .fn()
      .mockResolvedValue({ body: "theirs", bodyHash: "hash-9" });
    const out = await saveDraftBody({ save, reload }, loaded, "mine");
    expect(out).toEqual({
      kind: "conflict",
      theirs: "theirs",
      theirHash: "hash-9",
    });
    // The decision is the user's; nothing is written behind their back.
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("still reports a conflict when the reload also fails", async () => {
    const save = vi.fn().mockRejectedValue({ status: 409 });
    const reload = vi.fn().mockRejectedValue(new Error("offline"));
    const out = await saveDraftBody({ save, reload }, loaded, "mine");
    expect(out.kind).toBe("conflict");
  });

  it("surfaces a non-conflict failure as an error, not a conflict", async () => {
    const save = vi.fn().mockRejectedValue({ status: 500, message: "boom" });
    const out = await saveDraftBody({ save, reload: vi.fn() }, loaded, "mine");
    expect(out.kind).toBe("error");
  });

  it("recognises a conflict however the transport reports it", () => {
    expect(isConflict({ status: 409 })).toBe(true);
    expect(isConflict(new Error("Request failed with status 409"))).toBe(true);
    expect(isConflict(new Error("body conflict for 01ABC"))).toBe(true);
    expect(isConflict({ status: 500 })).toBe(false);
    expect(isConflict(new Error("network down"))).toBe(false);
  });
});

describe("createDebouncer", () => {
  it("collapses rapid edits into one save", async () => {
    vi.useFakeTimers();
    const fn = vi.fn();
    const d = createDebouncer(500);
    d.schedule(fn);
    d.schedule(fn);
    d.schedule(fn);
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(500);
    expect(fn).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it("cancel stops a pending save from firing after unmount", () => {
    vi.useFakeTimers();
    const fn = vi.fn();
    const d = createDebouncer(500);
    d.schedule(fn);
    expect(d.pending).toBe(true);
    d.cancel();
    vi.advanceTimersByTime(1000);
    expect(fn).not.toHaveBeenCalled();
    expect(d.pending).toBe(false);
    vi.useRealTimers();
  });
});
