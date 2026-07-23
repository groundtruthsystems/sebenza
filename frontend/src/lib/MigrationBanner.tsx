import { useEffect, useState } from "react";
import { fetchInstances } from "./api";
import type { InstanceSummary } from "./types";

// Other Sebenza servers running on this machine (migration sensor). Sebenza now
// runs one multi-project server per machine, so any peer here is a leftover
// single-project instance the user should fold in with `sebenza-cli project migrate`.
export default function MigrationBanner() {
  const [others, setOthers] = useState<InstanceSummary[]>([]);
  const [dismissed, setDismissed] = useState(false);

  const summary = others.map((other) => other.projectDir).join(", ");

  useEffect(() => {
    async function load(): Promise<void> {
      try {
        setOthers(await fetchInstances());
      } catch {
        setOthers([]);
      }
    }
    void load();
  }, []);

  if (!(others.length > 0 && !dismissed)) return null;

  return (
    <div className="flex items-start gap-3 px-4 py-2 text-[13px] bg-surface border-b border-edge text-primary">
      <div className="flex-1 min-w-0">
        <span className="text-amber-400 font-medium">
          {others.length} other Sebenza {others.length === 1 ? "server is" : "servers are"} running
        </span>{" "}
        <span className="text-muted">({summary}).</span>{" "}
        Consolidate {others.length === 1 ? "it" : "them"} into this dashboard — run{" "}
        <code className="text-primary">sebenza-cli project migrate</code> in your terminal.
      </div>
      <button
        type="button"
        className="shrink-0 px-1 text-muted hover:text-primary"
        aria-label="Dismiss"
        onClick={() => setDismissed(true)}
      >
        ×
      </button>
    </div>
  );
}
