import { useEffect, useState, type KeyboardEvent } from "react";
import { setUpProject } from "./api";
import { applyTheme, loadSavedTheme, projectInitPhaseLabel } from "./utils";
import type { ProjectInitPhase } from "./types";

export default function EmptyProjects() {
  const [path, setPath] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [phase, setPhase] = useState<ProjectInitPhase | null>(null);

  useEffect(() => {
    applyTheme(loadSavedTheme());
  }, []);

  async function add(): Promise<void> {
    const target = path.trim();
    if (!target || busy) return;
    setBusy(true);
    setError(null);
    setPhase(null);
    try {
      const { prefix } = await setUpProject(target, (next) => setPhase(next));
      window.location.assign(`/${prefix}/`);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
      setPhase(null);
    }
  }

  function onKeydown(event: KeyboardEvent): void {
    if (event.key === "Enter") {
      event.preventDefault();
      void add();
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-surface text-primary p-6">
      <div className="w-full max-w-md">
        <h1 className="text-lg font-semibold mb-2">No projects yet</h1>
        <p className="text-sm text-muted mb-4">
          Sebenza serves every project from this one dashboard. Add a git repo below
          and Sebenza sets it up for you — scaffolding a{" "}
          <code className="text-primary">.ai/sebenza.yaml</code> and analyzing the project
          to fill it in.
        </p>
        <div className="flex gap-2">
          <input
            type="text"
            value={path}
            onChange={(e) => setPath(e.target.value)}
            onKeyDown={onKeydown}
            placeholder="Path to a git repo"
            disabled={busy}
            className="flex-1 min-w-0 px-3 py-2 text-sm rounded border border-edge bg-surface text-primary placeholder:text-muted disabled:opacity-50"
          />
          <button
            type="button"
            className="shrink-0 px-3 py-2 text-sm rounded border border-edge text-primary hover:bg-hover disabled:opacity-50"
            disabled={busy || path.trim() === ""}
            onClick={add}
          >
            Add
          </button>
        </div>
        {busy && phase && (
          <div className="mt-3 flex items-center gap-2 text-sm text-muted">
            <span className="spinner"></span>
            {projectInitPhaseLabel(phase)}…
          </div>
        )}
        {error && <div className="mt-2 text-sm text-red-400 break-words">{error}</div>}
        <p className="mt-6 text-sm text-muted">
          Already using the sebenza plugin elsewhere? The{" "}
          <a href="/registry" className="text-accent hover:underline">
            registry
          </a>{" "}
          shows tracks across every project it knows about.
        </p>
      </div>
    </div>
  );
}
