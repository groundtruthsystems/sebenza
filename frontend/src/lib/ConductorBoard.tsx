import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import type { ConductorTrack, ConductorTracks, WorktreeInfo } from "./types";
import { fetchConductorTracks } from "./api";
import { errorMessage } from "./utils";
import ConductorTrackDetail from "./ConductorTrackDetail";
import { CONDUCTOR_COLUMNS, statusDotClass } from "./conductorStatus";
import "./Conductor.css";

const POLL_MS = 4000;

function Centered({ className = "", children }: { className?: string; children: ReactNode }) {
  return (
    <div className={`flex-1 flex items-center justify-center text-sm px-6 text-center ${className}`}>
      {children}
    </div>
  );
}

function TrackCard({ track, onClick }: { track: ConductorTrack; onClick: () => void }) {
  const total = track.progress?.total_tasks ?? 0;
  const done = track.progress?.completed_tasks ?? 0;
  const pct = Math.round(track.progress?.percentage ?? 0);
  return (
    <button
      type="button"
      onClick={onClick}
      className="w-full text-left rounded-lg border border-edge bg-surface hover:border-accent p-3 cursor-pointer transition-colors"
    >
      {track.type && <span className="text-[10px] uppercase text-muted">{track.type}</span>}
      <p className="text-[13px] text-primary mt-0.5 line-clamp-3">{track.description}</p>
      <div className="mt-2 h-1.5 rounded-full bg-hover overflow-hidden">
        <div className="h-full bg-accent" style={{ width: `${pct}%` }} />
      </div>
      <div className="mt-1 text-[10px] text-muted">
        {done}/{total} ({pct}%)
      </div>
      {track.blocked_reason && (
        <p className="mt-1 text-[10px] text-danger">Blocked: {track.blocked_reason}</p>
      )}
      {track.phases_summary && track.phases_summary.length > 0 && (
        <div className="mt-2 space-y-0.5">
          {track.phases_summary.map((ph) => (
            <div key={ph.id} className="flex items-center gap-1.5 text-[10px] text-muted">
              <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${statusDotClass(ph.status)}`} />
              <span className="truncate">{ph.name}</span>
            </div>
          ))}
        </div>
      )}
    </button>
  );
}

export default function ConductorBoard({ worktree }: { worktree: WorktreeInfo }) {
  const branch = worktree.branch;
  const [tracks, setTracks] = useState<ConductorTracks | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState<ConductorTrack | null>(null);
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
      <div className="flex gap-3 p-4 min-h-full items-start">
        {CONDUCTOR_COLUMNS.map((col) => {
          const cards = list.filter((t) => t.status === col.key);
          return (
            <div key={col.key} className="flex-1 min-w-[210px] flex flex-col">
              <div className="flex items-center gap-2 mb-2 px-1">
                <span className={`w-2 h-2 rounded-full ${statusDotClass(col.key)}`} />
                <span className="text-[12px] text-primary">{col.label}</span>
                <span className="text-[11px] text-muted">{cards.length}</span>
              </div>
              <div className="space-y-2">
                {cards.map((t) => (
                  <TrackCard key={t.track_id} track={t} onClick={() => setSelected(t)} />
                ))}
              </div>
            </div>
          );
        })}
      </div>

      {selected && (
        <ConductorTrackDetail branch={branch} track={selected} onclose={() => setSelected(null)} />
      )}
    </div>
  );
}
