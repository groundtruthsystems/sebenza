import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import type { Track, Tracks, WorktreeInfo } from "./types";
import { fetchTrackFile, fetchTracks } from "./api";
import { errorMessage } from "./utils";
import TrackDetail from "./TrackDetail";
import PhaseDetail from "./PhaseDetail";
import TrackGroup, { type SelectedPhase } from "./TrackGroup";
import "./Tracks.css";

const POLL_MS = 4000;

function Centered({ className = "", children }: { className?: string; children: ReactNode }) {
  return (
    <div className={`flex-1 flex items-center justify-center text-sm px-6 text-center ${className}`}>
      {children}
    </div>
  );
}

/** Kanban board over a worktree's `.ai/sebenza/tracks.json` — the workspace
 *  written by the `sebenza` plugin. */
export default function TracksBoard({ worktree }: { worktree: WorktreeInfo }) {
  const branch = worktree.branch;
  const [tracks, setTracks] = useState<Tracks | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [selectedPhase, setSelectedPhase] = useState<SelectedPhase | null>(null);
  const [docsTrack, setDocsTrack] = useState<Track | null>(null);
  const loadedOnce = useRef(false);

  // Stable per-branch reader, so the detail views' effects don't re-fire.
  const fetchFile = useCallback((path: string) => fetchTrackFile(branch, path), [branch]);

  const load = useCallback(
    (initial: boolean) => {
      if (initial) setLoading(true);
      fetchTracks(branch)
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
    return (
      <Centered className="text-muted">
        <span>
          No Sebenza tracks for this worktree.
          <br />
          <span className="text-[12px]">
            Tracks live in <code>.ai/sebenza/</code>, created by the sebenza plugin&rsquo;s setup
            skill.
          </span>
        </span>
      </Centered>
    );

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
        <PhaseDetail
          fetchFile={fetchFile}
          planPath={selectedPhase.planPath}
          phaseId={selectedPhase.phaseId}
          phaseName={selectedPhase.phaseName}
          onclose={() => setSelectedPhase(null)}
        />
      )}
      {docsTrack && (
        <TrackDetail fetchFile={fetchFile} track={docsTrack} onclose={() => setDocsTrack(null)} />
      )}
    </div>
  );
}
