/**
 * Save logic for the draft editor, kept out of the component so the rules that
 * matter — what counts as a conflict, and what a force-write may touch — are
 * testable without rendering anything.
 */

export type SaveOutcome =
  | { kind: "saved"; bodyHash: string }
  | { kind: "unchanged" }
  /** The file moved under us. `theirs` is what is on disk now. */
  | { kind: "conflict"; theirs: string; theirHash: string }
  | { kind: "error"; message: string };

export interface DraftLike {
  body: string;
  bodyHash: string;
}

export interface SaveDeps {
  /** PUT the body; rejects with a 409-shaped error on a stale hash. */
  save: (expectedHash: string, body: string) => Promise<{ bodyHash: string }>;
  /** Re-read the draft, to show what the other writer put there. */
  reload: () => Promise<DraftLike>;
}

/** A 409 from the body route, however the transport surfaces it. */
export function isConflict(err: unknown): boolean {
  if (typeof err === "object" && err !== null) {
    const status = (err as { status?: number }).status;
    if (status === 409) return true;
  }
  return /\b409\b|conflict/i.test(String((err as Error)?.message ?? err));
}

/**
 * Save `body` against the hash it was loaded at.
 *
 * A conflict is reported, never resolved silently: the caller decides between
 * reloading and force-writing. That choice is the user's because only they know
 * whether the other writer's change matters.
 */
export async function saveDraftBody(
  deps: SaveDeps,
  loaded: DraftLike,
  body: string,
): Promise<SaveOutcome> {
  if (body === loaded.body) return { kind: "unchanged" };
  try {
    const { bodyHash } = await deps.save(loaded.bodyHash, body);
    return { kind: "saved", bodyHash };
  } catch (err) {
    if (!isConflict(err)) {
      return { kind: "error", message: (err as Error)?.message ?? String(err) };
    }
    try {
      const theirs = await deps.reload();
      return { kind: "conflict", theirs: theirs.body, theirHash: theirs.bodyHash };
    } catch {
      return {
        kind: "conflict",
        theirs: "",
        theirHash: "",
      };
    }
  }
}

/**
 * Debounce a save. Returns a `schedule` that restarts the timer and a `cancel`
 * for unmount, so a pending autosave cannot fire against a closed editor.
 */
export function createDebouncer(delayMs: number) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return {
    schedule(fn: () => void) {
      if (timer !== undefined) clearTimeout(timer);
      timer = setTimeout(fn, delayMs);
    },
    cancel() {
      if (timer !== undefined) clearTimeout(timer);
      timer = undefined;
    },
    get pending() {
      return timer !== undefined;
    },
  };
}
