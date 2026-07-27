import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { fetchProjects, removeProject, setUpProject } from "./api";
import { projectInitPhaseLabel } from "./utils";
import type { ProjectInitPhase, ProjectSummary } from "./types";

export default function ProjectSwitcher({ current }: { current: string }) {
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [open, setOpen] = useState(false);
  const [addPath, setAddPath] = useState("");
  const [addError, setAddError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [addPhase, setAddPhase] = useState<ProjectInitPhase | null>(null);
  const [menuRect, setMenuRect] = useState<{ top: number; left: number; width: number }>({
    top: 0,
    left: 0,
    width: 0,
  });
  const triggerEl = useRef<HTMLButtonElement>(null);
  const menuEl = useRef<HTMLDivElement>(null);

  const load = useCallback(async (): Promise<void> => {
    try {
      setProjects(await fetchProjects());
    } catch {
      setProjects([]);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const positionMenu = useCallback((): void => {
    if (!triggerEl.current) return;
    const rect = triggerEl.current.getBoundingClientRect();
    const width = Math.max(rect.width + 120, 280);
    const left = Math.max(8, Math.min(rect.left, window.innerWidth - width - 8));
    setMenuRect({ top: rect.bottom + 4, left, width });
  }, []);

  function toggle(): void {
    if (open) {
      setOpen(false);
      return;
    }
    setAddError(null);
    void load();
    positionMenu();
    setOpen(true);
  }

  useEffect(() => {
    function handleDocumentClick(event: globalThis.MouseEvent): void {
      if (!open) return;
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (triggerEl.current?.contains(target)) return;
      if (menuEl.current?.contains(target)) return;
      setOpen(false);
    }

    function handleKeydown(event: globalThis.KeyboardEvent): void {
      if (open && event.key === "Escape") {
        setOpen(false);
        triggerEl.current?.focus();
      }
    }

    const handleReposition = (): void => {
      if (open) positionMenu();
    };

    window.addEventListener("click", handleDocumentClick);
    window.addEventListener("keydown", handleKeydown);
    window.addEventListener("resize", handleReposition);
    window.addEventListener("scroll", handleReposition);
    return () => {
      window.removeEventListener("click", handleDocumentClick);
      window.removeEventListener("keydown", handleKeydown);
      window.removeEventListener("resize", handleReposition);
      window.removeEventListener("scroll", handleReposition);
    };
  }, [open, positionMenu]);

  async function handleAdd(): Promise<void> {
    const path = addPath.trim();
    if (!path || busy) return;
    setBusy(true);
    setAddError(null);
    setAddPhase(null);
    try {
      const { prefix } = await setUpProject(path, (next) => setAddPhase(next));
      window.location.assign(`/${prefix}/`);
    } catch (error) {
      setAddError(error instanceof Error ? error.message : String(error));
      setBusy(false);
      setAddPhase(null);
    }
  }

  function handleAddKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter") {
      event.preventDefault();
      void handleAdd();
    }
  }

  async function handleRemove(event: MouseEvent, prefix: string): Promise<void> {
    event.preventDefault();
    event.stopPropagation();
    if (busy) return;
    setBusy(true);
    try {
      await removeProject(prefix);
      await load();
    } catch {
      // ignore — list reload will reflect actual state
    }
    setBusy(false);
  }

  return (
    <>
      <button
        ref={triggerEl}
        type="button"
        className="shrink-0 h-6 w-6 inline-flex items-center justify-center rounded-md text-muted hover:bg-hover hover:text-primary"
        title="Switch project"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={toggle}
      >
        <svg viewBox="0 0 12 12" width="10" height="10" aria-hidden="true">
          <path
            d="M2 4 L6 8 L10 4"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </button>

      {open && (
        <div
          ref={menuEl}
          role="menu"
          className="fixed z-50 rounded-md border border-edge bg-surface shadow-lg overflow-hidden"
          style={{ top: menuRect.top, left: menuRect.left, width: menuRect.width }}
        >
          <div className="px-3 py-2 text-[11px] text-muted uppercase tracking-wide border-b border-edge">
            Projects
          </div>
          {projects.map((project) => (
            <div
              key={project.prefix}
              className="flex items-stretch border-t border-edge first:border-t-0 hover:bg-hover"
            >
              <a
                href={`/${project.prefix}/`}
                className="block flex-1 min-w-0 px-3 py-2 text-[12px]"
                role="menuitem"
              >
                <div className="text-primary font-medium truncate">
                  {project.name}
                  {project.prefix === current && (
                    <span className="text-muted text-[10px] font-normal"> · current</span>
                  )}
                </div>
                <div className="text-muted text-[11px] truncate">{project.path}</div>
              </a>
              {project.prefix !== current && (
                <button
                  type="button"
                  className="shrink-0 px-2 text-muted hover:text-primary"
                  title="Remove project"
                  disabled={busy}
                  onClick={(event) => handleRemove(event, project.prefix)}
                >
                  ×
                </button>
              )}
            </div>
          ))}

          <div className="px-3 py-2 border-t border-edge">
            <div className="flex gap-1">
              <input
                type="text"
                value={addPath}
                onChange={(e) => setAddPath(e.target.value)}
                onKeyDown={handleAddKeydown}
                placeholder="Path to a git repo…"
                disabled={busy}
                className="flex-1 min-w-0 px-2 py-1 text-[12px] rounded border border-edge bg-surface text-primary placeholder:text-muted disabled:opacity-50"
              />
              <button
                type="button"
                className="shrink-0 px-2 py-1 text-[12px] rounded border border-edge text-primary hover:bg-hover disabled:opacity-50"
                disabled={busy || addPath.trim() === ""}
                onClick={handleAdd}
              >
                Add
              </button>
            </div>
            {busy && addPhase && (
              <div className="mt-1 flex items-center gap-1 text-[11px] text-muted">
                <span className="spinner"></span>
                {projectInitPhaseLabel(addPhase)}…
              </div>
            )}
            {addError && <div className="mt-1 text-[11px] text-red-400 break-words">{addError}</div>}
          </div>

          <a
            href="/registry"
            role="menuitem"
            className="block px-3 py-2 text-[12px] border-t border-edge hover:bg-hover"
          >
            <div className="text-primary">Sebenza registry</div>
            <div className="text-muted text-[11px]">Tracks across every registered project</div>
          </a>
        </div>
      )}
    </>
  );
}
