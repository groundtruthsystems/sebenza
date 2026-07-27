import type { PhaseSummary, Track } from "./types";
import { TRACK_COLUMNS, statusDotClass } from "./trackStatus";

/** A phase the user picked off a board card, carrying enough to fetch its plan. */
export interface SelectedPhase {
  planPath: string | undefined;
  phaseId: string;
  phaseName: string;
}

function PhaseCard({ phase, onClick }: { phase: PhaseSummary; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="w-full text-left rounded-md border border-edge bg-surface hover:border-accent p-2 cursor-pointer transition-colors"
    >
      <div className="flex items-start gap-1.5">
        <span className={`mt-1 w-1.5 h-1.5 rounded-full shrink-0 ${statusDotClass(phase.status)}`} />
        <span className="text-[12px] text-primary">{phase.name}</span>
      </div>
      {phase.blocked_reason && (
        <p className="mt-1 pl-3 text-[10px] text-danger">{phase.blocked_reason}</p>
      )}
    </button>
  );
}

/** One track rendered as a titled kanban strip of its phases. Shared by the
 *  per-worktree board and the cross-project registry portfolio. */
export default function TrackGroup({
  track,
  onPhaseClick,
  onView,
}: {
  track: Track;
  onPhaseClick: (phase: SelectedPhase) => void;
  onView: () => void;
}) {
  const phases = track.phases_summary ?? [];
  const doneCount = phases.filter((p) => p.status === "done").length;
  const allDone = phases.length > 0 && doneCount === phases.length;
  const hasDocs = !!track.spec_path || !!track.design_path;

  return (
    <div className="rounded-lg border border-edge">
      <div className="flex flex-wrap items-center gap-2 px-3 py-2 border-b border-edge">
        {track.type && (
          <span className="text-[10px] uppercase px-1.5 py-0.5 rounded border border-edge text-muted">
            {track.type}
          </span>
        )}
        <span className="text-[13px] text-primary">{track.description}</span>
        <span className="text-[11px] text-muted">
          {doneCount}/{phases.length} phases
        </span>
        {allDone && (
          <span className="text-[10px] px-1.5 py-0.5 rounded border border-success/40 text-success">
            Complete
          </span>
        )}
        {hasDocs && (
          <button
            type="button"
            onClick={onView}
            className="ml-auto text-[11px] px-2 py-0.5 rounded-md border border-edge text-accent hover:bg-hover cursor-pointer"
          >
            View
          </button>
        )}
      </div>

      {phases.length === 0 ? (
        <p className="px-3 py-3 text-[11px] text-muted">
          No phases yet — this track has a design but no plan.
        </p>
      ) : (
        <div className="flex gap-3 p-3 overflow-x-auto items-start">
          {TRACK_COLUMNS.map((col) => {
            const cards = phases.filter((p) => p.status === col.key);
            return (
              <div key={col.key} className="flex-1 min-w-[180px] flex flex-col">
                <div className="flex items-center gap-2 mb-2 px-0.5">
                  <span className={`w-2 h-2 rounded-full ${statusDotClass(col.key)}`} />
                  <span className="text-[11px] text-primary">{col.label}</span>
                  <span className="text-[10px] text-muted">{cards.length}</span>
                </div>
                <div className="space-y-2">
                  {cards.map((phase) => (
                    <PhaseCard
                      key={phase.id}
                      phase={phase}
                      onClick={() =>
                        onPhaseClick({
                          planPath: track.plan_path,
                          phaseId: phase.id,
                          phaseName: phase.name,
                        })
                      }
                    />
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
