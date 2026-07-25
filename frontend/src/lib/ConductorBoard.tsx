import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import type {
  ConductorPhaseSummary,
  ConductorTrack,
  ConductorTracks,
  WorktreeInfo,
} from "./types";
import { fetchConductorTracks } from "./api";
import { errorMessage } from "./utils";
import ConductorTrackDetail from "./ConductorTrackDetail";
import ConductorPhaseDetail from "./ConductorPhaseDetail";
import { CONDUCTOR_COLUMNS, statusDotClass } from "./conductorStatus";
import "./Conductor.css";

const POLL_MS = 4000;

interface SelectedPhase {
  planPath: string | undefined;
  phaseId: string;
  phaseName: string;
}

function Centered({ className = "", children }: { className?: string; children: ReactNode }) {
  return (
    <div className={`flex-1 flex items-center justify-center text-sm px-6 text-center ${className}`}>
      {children}
    </div>
  );
}

function PhaseCard({ phase, onClick }: { phase: ConductorPhaseSummary; onClick: () => void }) {
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

function TrackGroup({
  track,
  onPhaseClick,
  onView,
}: {
  track: ConductorTrack;
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

      <div className="flex gap-3 p-3 overflow-x-auto items-start">
        {CONDUCTOR_COLUMNS.map((col) => {
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
    </div>
  );
}

export default function ConductorBoard({ worktree }: { worktree: WorktreeInfo }) {
  const branch = worktree.branch;
  const [tracks, setTracks] = useState<ConductorTracks | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [selectedPhase, setSelectedPhase] = useState<SelectedPhase | null>(null);
  const [docsTrack, setDocsTrack] = useState<ConductorTrack | null>(null);
  const loadedOnce = useRef(false);

  const load = useCallback(
    (initial: boolean) => {
      if (initial) setLoading(true);
      fetchConductorTracks(branch)
        .then((res) => {
          setTracks(res);
          setError("");
        })
        .catch((err: unknown) => {
          // Only surface poll errors before the first successful load.
          if (initial || !loadedOnce.current) setError(errorMessage(err));
        })
        .finally(() => {
          loadedOnce.current = true;
          if (initial) setLoading(false);
        });
    },
    [branch],
  );

  useEffect(() => {
    loadedOnce.current = false;
    load(true);
    const id = setInterval(() => load(false), POLL_MS);
    return () => clearInterval(id);
  }, [load]);

  if (loading) return <Centered className="text-muted">Loading tracks…</Centered>;
  if (error) return <Centered className="text-danger">{error}</Centered>;

  const list = tracks?.tracks ?? [];
  if (list.length === 0)
    return <Centered className="text-muted">No conductor tracks for this worktree.</Centered>;

  return (
    <div className="flex-1 overflow-auto">
      <div className="p-4 space-y-4">
        {list.map((track) => (
          <TrackGroup
            key={track.track_id}
            track={track}
            onPhaseClick={setSelectedPhase}
            onView={() => setDocsTrack(track)}
          />
        ))}
      </div>

      {selectedPhase && (
        <ConductorPhaseDetail
          branch={branch}
          planPath={selectedPhase.planPath}
          phaseId={selectedPhase.phaseId}
          phaseName={selectedPhase.phaseName}
          onclose={() => setSelectedPhase(null)}
        />
      )}
      {docsTrack && (
        <ConductorTrackDetail
          branch={branch}
          track={docsTrack}
          onclose={() => setDocsTrack(null)}
        />
      )}
    </div>
  );
}
