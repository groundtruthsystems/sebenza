import { useEffect, useRef, useState, type FormEvent } from "react";
import { fetchBaseBranchesFor } from "./api";
import BaseDialog from "./BaseDialog";
import Btn from "./Btn";
import type { ProjectSummary } from "./types";

/** One row of the fan-out. `key` is local only — the server never sees it. */
export interface TargetDraft {
  key: number;
  projectPath: string;
  branch: string;
  /** Empty means "the project's default", resolved server-side. */
  baseBranch: string;
  prompt: string;
}

export interface ConvertTarget {
  projectPath: string;
  branch: string;
  baseBranch?: string;
  prompt: string;
  /** The reviewed system instruction; absent launches with `prompt` alone. */
  systemInstruction?: string;
  architectFirst?: boolean;
}

/** Matches the server's cap; exceeding it is refused there too. */
export const MAX_TARGETS = 10;

let nextKey = 1;

function emptyTarget(projectPath: string): TargetDraft {
  return { key: nextKey++, projectPath, branch: "", baseBranch: "", prompt: "" };
}

/**
 * Local validation, mirroring the server's rules closely enough to catch the
 * obvious mistakes before a round trip. The server re-validates regardless —
 * this exists to point at the offending row, not to be the gate.
 */
export function rowError(
  target: TargetDraft,
  all: TargetDraft[],
): string | null {
  if (!target.projectPath) return "Pick a project";
  if (!target.branch.trim()) return "Branch is required";
  if (/\s/.test(target.branch.trim())) return "Branch cannot contain spaces";
  if (/\s/.test(target.baseBranch.trim()))
    return "Source branch cannot contain spaces";
  if (!target.prompt.trim()) return "Prompt is required";
  const duplicate = all.some(
    (other) =>
      other.key !== target.key &&
      other.projectPath === target.projectPath &&
      other.branch.trim() === target.branch.trim() &&
      other.branch.trim() !== "",
  );
  // Git would refuse the second one; better to say so before creating the first.
  if (duplicate) return "Same branch twice in this project";
  return null;
}

export default function ConvertDraftDialog({
  draftTitle,
  draftId: _draftId,
  projects,
  previousTargets,
  loading = false,
  error = "",
  onconvert,
  oncancel,
}: {
  draftTitle: string;
  /** Enables "Draft instructions" (the system agent's per-target
   *  instruction). Absent, the dialog converts with prompts alone. */
  draftId?: string;
  projects: ProjectSummary[];
  /** The previous wave, so a second conversion starts from what was done
   *  before rather than a blank form. */
  previousTargets?: ConvertTarget[];
  loading?: boolean;
  error?: string;
  onconvert: (targets: ConvertTarget[]) => void;
  oncancel: () => void;
}) {
  const defaultProject = projects[0]?.path ?? "";
  const [targets, setTargets] = useState<TargetDraft[]>(() =>
    previousTargets && previousTargets.length > 0
      ? previousTargets.map((t) => ({
          ...t,
          baseBranch: t.baseBranch ?? "",
          key: nextKey++,
        }))
      : [emptyTarget(defaultProject)],
  );
  /** Base branches per project path, fetched lazily as rows point at them. */
  const [branchesByProject, setBranchesByProject] = useState<
    Record<string, string[]>
  >({});
  const firstBranch = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const el = firstBranch.current;
    if (!el) return;
    queueMicrotask(() => el.focus());
  }, []);

  // Each row offers its own project's branches, because targets routinely
  // span projects that share no branch names at all.
  useEffect(() => {
    const wanted = new Set(targets.map((t) => t.projectPath).filter(Boolean));
    for (const path of wanted) {
      if (branchesByProject[path]) continue;
      const prefix = projects.find((p) => p.path === path)?.prefix;
      if (!prefix) continue;
      void fetchBaseBranchesFor(prefix)
        .then((names) =>
          setBranchesByProject((prev) => ({ ...prev, [path]: names })),
        )
        // A project whose branches cannot be listed still converts; the field
        // just falls back to free text.
        .catch(() =>
          setBranchesByProject((prev) => ({ ...prev, [path]: [] })),
        );
    }
  }, [targets, projects, branchesByProject]);

  const errors = targets.map((t) => rowError(t, targets));
  const canConvert =
    !loading && targets.length > 0 && errors.every((e) => e === null);

  const update = (key: number, patch: Partial<TargetDraft>) =>
    setTargets((prev) =>
      prev.map((t) => (t.key === key ? { ...t, ...patch } : t)),
    );

  return (
    <BaseDialog onclose={oncancel} maxWidth="720px">
      <form
        onSubmit={(event: FormEvent) => {
          event.preventDefault();
          if (!canConvert) return;
          onconvert(
            targets.map(({ projectPath, branch, baseBranch, prompt }) => ({
              projectPath,
              branch: branch.trim(),
              // Omitted rather than sent empty: absent means "the project's
              // default", which the server resolves.
              ...(baseBranch.trim() ? { baseBranch: baseBranch.trim() } : {}),
              prompt: prompt.trim(),
            })),
          );
        }}
      >
        <h2 className="text-base mb-1">Convert to worktrees</h2>
        <p className="text-[12px] text-muted mb-4">
          Each worktree gets its own prompt and a copy of “{draftTitle}”.
        </p>

        {projects.length === 0 && (
          <p className="text-[12px] text-danger mb-4">
            No projects are registered, so there is nowhere to create a worktree.
          </p>
        )}

        <div className="flex flex-col gap-3 max-h-[46vh] overflow-y-auto pr-1">
          {targets.map((target, index) => (
            <div
              key={target.key}
              className="rounded-md border border-edge p-3 flex flex-col gap-2"
            >
              <div className="flex items-center gap-2">
                <select
                  aria-label={`Project for target ${index + 1}`}
                  className="h-7 min-w-0 flex-1 rounded-md border border-edge bg-surface px-2 text-xs text-primary focus:outline-none focus:border-accent"
                  value={target.projectPath}
                  onChange={(e) =>
                    update(target.key, { projectPath: e.currentTarget.value })
                  }
                  disabled={loading}
                >
                  {projects.map((p) => (
                    <option key={p.path} value={p.path}>
                      {p.name}
                    </option>
                  ))}
                </select>
                <input
                  ref={index === 0 ? firstBranch : undefined}
                  aria-label={`Branch for target ${index + 1}`}
                  placeholder="branch-name"
                  className="h-7 w-[200px] rounded-md border border-edge bg-surface px-2 text-xs text-primary placeholder:text-muted focus:outline-none focus:border-accent"
                  value={target.branch}
                  onChange={(e) =>
                    update(target.key, { branch: e.currentTarget.value })
                  }
                  disabled={loading}
                />
                <input
                  aria-label={`Source branch for target ${index + 1}`}
                  list={`base-branches-${target.projectPath}`}
                  placeholder="from (default)"
                  title="Branch to fork from; blank uses the project's default"
                  className="h-7 w-[150px] rounded-md border border-edge bg-surface px-2 text-xs text-primary placeholder:text-muted focus:outline-none focus:border-accent"
                  value={target.baseBranch}
                  onChange={(e) =>
                    update(target.key, { baseBranch: e.currentTarget.value })
                  }
                  disabled={loading}
                />
                <button
                  type="button"
                  aria-label={`Remove target ${index + 1}`}
                  title="Remove"
                  className="h-7 w-7 shrink-0 rounded-md border border-edge text-muted hover:text-primary hover:bg-hover disabled:opacity-40 cursor-pointer"
                  onClick={() =>
                    setTargets((prev) => prev.filter((t) => t.key !== target.key))
                  }
                  disabled={loading || targets.length === 1}
                >
                  &times;
                </button>
              </div>
              <datalist id={`base-branches-${target.projectPath}`}>
                {(branchesByProject[target.projectPath] ?? []).map((b) => (
                  <option key={b} value={b} />
                ))}
              </datalist>
              <textarea
                aria-label={`Prompt for target ${index + 1}`}
                placeholder="What should this worktree's agent do?"
                rows={2}
                className="w-full rounded-md border border-edge bg-surface px-2 py-1.5 text-xs text-primary placeholder:text-muted focus:outline-none focus:border-accent resize-y"
                value={target.prompt}
                onChange={(e) =>
                  update(target.key, { prompt: e.currentTarget.value })
                }
                disabled={loading}
              />
              {errors[index] && (
                <p className="text-[11px] text-danger m-0">{errors[index]}</p>
              )}
            </div>
          ))}
        </div>

        <div className="mt-3 flex items-center justify-between gap-2">
          <Btn
            type="button"
            small
            onClick={() =>
              setTargets((prev) => [...prev, emptyTarget(defaultProject)])
            }
            disabled={loading || targets.length >= MAX_TARGETS || projects.length === 0}
            title={
              targets.length >= MAX_TARGETS
                ? `At most ${MAX_TARGETS} worktrees per conversion`
                : undefined
            }
          >
            + Add worktree
          </Btn>
          <span className="text-[11px] text-muted">
            {targets.length} of {MAX_TARGETS}
          </span>
        </div>

        {error && (
          <p className="text-[12px] text-danger mt-3 whitespace-pre-wrap">
            {error}
          </p>
        )}

        <div className="flex justify-end gap-2 mt-4">
          <Btn type="button" onClick={oncancel} disabled={loading}>
            Cancel
          </Btn>
          <Btn
            type="submit"
            variant="cta"
            className="flex items-center gap-1.5"
            disabled={!canConvert}
          >
            {loading && <span className="spinner"></span>}
            Create {targets.length} worktree{targets.length === 1 ? "" : "s"}
          </Btn>
        </div>
      </form>
    </BaseDialog>
  );
}
