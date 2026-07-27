import { useCallback, useEffect, useMemo, useState } from "react";
import type { Portfolio, RegistryProject, Track, TrackStatus } from "./types";
import { fetchRegistry, fetchRegistryFile } from "./api";
import { errorMessage } from "./utils";
import TrackDetail from "./TrackDetail";
import PhaseDetail from "./PhaseDetail";
import TrackGroup, { type SelectedPhase } from "./TrackGroup";
import { TRACK_COLUMNS, statusDotClass, statusTextClass } from "./trackStatus";
import "./Tracks.css";

/** A track plus the project it came from, so the portfolio can label it. */
interface Blocker {
  project: string;
  trackId: string;
  phase: string | null;
  reason: string;
}

function tracksOf(project: RegistryProject): Track[] {
  return project.tracks?.tracks ?? [];
}

/** Aggregate every track across every healthy project: per-status counts plus
 *  overall task completion, mirroring the plugin's PORTFOLIO KANBAN summary. */
function rollUp(projects: RegistryProject[]) {
  const byStatus = new Map<TrackStatus, number>(TRACK_COLUMNS.map((c) => [c.key, 0]));
  let totalTasks = 0;
  let completedTasks = 0;
  let trackCount = 0;

  for (const project of projects) {
    for (const track of tracksOf(project)) {
      trackCount++;
      byStatus.set(track.status, (byStatus.get(track.status) ?? 0) + 1);
      totalTasks += track.progress?.total_tasks ?? 0;
      completedTasks += track.progress?.completed_tasks ?? 0;
    }
  }

  const percentage = totalTasks > 0 ? Math.round((completedTasks / totalTasks) * 100) : 0;
  return { byStatus, totalTasks, completedTasks, percentage, trackCount };
}

/** Every blocked track and phase across the portfolio. The plugin treats
 *  surfacing blockers as the headline output of a status report. */
function collectBlockers(projects: RegistryProject[]): Blocker[] {
  const out: Blocker[] = [];
  for (const project of projects) {
    for (const track of tracksOf(project)) {
      if (track.blocked_reason) {
        out.push({
          project: project.name,
          trackId: track.track_id,
          phase: null,
          reason: track.blocked_reason,
        });
      }
      for (const phase of track.phases_summary ?? []) {
        if (phase.blocked_reason) {
          out.push({
            project: project.name,
            trackId: track.track_id,
            phase: phase.name,
            reason: phase.blocked_reason,
          });
        }
      }
    }
  }
  return out;
}

const STATUS_NOTE: Record<string, string> = {
  missing_path: "Project directory not found",
  missing_tracks: "No readable .ai/sebenza/tracks.json",
  invalid_tracks: "tracks.json is not valid JSON",
};

function ProjectSection({ project }: { project: RegistryProject }) {
  const [selectedPhase, setSelectedPhase] = useState<SelectedPhase | null>(null);
  const [docsTrack, setDocsTrack] = useState<Track | null>(null);

  // Stable per-project reader, so the detail views' effects don't re-fire.
  const fetchFile = useCallback(
    (path: string) => fetchRegistryFile(project.path, path),
    [project.path],
  );

  const tracks = tracksOf(project);
  const unhealthy = project.status !== "ok";

  return (
    <section className="rounded-lg border border-edge">
      <header className="flex flex-wrap items-baseline gap-x-3 gap-y-1 px-3 py-2 border-b border-edge">
        <h2 className="text-[14px] text-primary">{project.name}</h2>
        <code className="text-[11px] text-muted">{project.path}</code>
        {!unhealthy && (
          <span className="ml-auto text-[11px] text-muted">
            {tracks.length} {tracks.length === 1 ? "track" : "tracks"}
          </span>
        )}
        {unhealthy && (
          <span className="ml-auto text-[11px] text-warning">
            {STATUS_NOTE[project.status] ?? project.status}
          </span>
        )}
      </header>

      {unhealthy ? (
        <p className="px-3 py-3 text-[12px] text-muted">
          Skipped in the rollup.
          {project.error && <span className="block font-mono text-[11px] mt-1">{project.error}</span>}
        </p>
      ) : tracks.length === 0 ? (
        <p className="px-3 py-3 text-[12px] text-muted">No tracks yet.</p>
      ) : (
        <div className="p-3 space-y-3">
          {tracks.map((track) => (
            <TrackGroup
              key={track.track_id}
              track={track}
              onPhaseClick={setSelectedPhase}
              onView={() => setDocsTrack(track)}
            />
          ))}
        </div>
      )}

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
    </section>
  );
}

/** The cross-project portfolio over `~/.ai/sebenza/registry.json`. Mounted at
 *  `/registry` — user-scoped, so it sits outside any project prefix. */
export default function RegistryView() {
  const [portfolio, setPortfolio] = useState<Portfolio | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  // Theme vars are already applied to :root by main.tsx before mount.

  // Registry contents change on human timescales and each load fans out to one
  // file read per project, so this refreshes on demand rather than polling.
  const load = useCallback(() => {
    setLoading(true);
    fetchRegistry()
      .then((res) => {
        setPortfolio(res);
        setError("");
      })
      .catch((err: unknown) => setError(errorMessage(err)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => load(), [load]);

  const healthy = useMemo(
    () => (portfolio?.projects ?? []).filter((p) => p.status === "ok"),
    [portfolio],
  );
  const summary = useMemo(() => rollUp(healthy), [healthy]);
  const blockers = useMemo(() => collectBlockers(healthy), [healthy]);

  return (
    <div className="min-h-screen bg-surface text-primary">
      <div className="max-w-5xl mx-auto p-6 space-y-5">
        <header className="flex flex-wrap items-center gap-3">
          <h1 className="text-lg font-semibold">Sebenza registry</h1>
          <code className="text-[11px] text-muted">{portfolio?.registry_path ?? ""}</code>
          <div className="ml-auto flex items-center gap-2">
            <a
              href="/"
              className="text-[12px] px-2 py-1 rounded-md border border-edge text-muted hover:text-primary hover:bg-hover"
            >
              Dashboard
            </a>
            <button
              type="button"
              onClick={load}
              disabled={loading}
              className="text-[12px] px-2 py-1 rounded-md border border-edge text-accent hover:bg-hover cursor-pointer disabled:opacity-40"
            >
              {loading ? "Refreshing…" : "Refresh"}
            </button>
          </div>
        </header>

        {error && <p className="text-sm text-danger">{error}</p>}

        {!error && portfolio && !portfolio.exists && (
          <p className="text-sm text-muted">
            No registry at <code>{portfolio.registry_path}</code>. It is created the first time the
            sebenza plugin&rsquo;s setup skill registers a project.
          </p>
        )}

        {!error && portfolio?.error && (
          <p className="text-sm text-danger">
            Registry could not be parsed: <span className="font-mono">{portfolio.error}</span>
          </p>
        )}

        {portfolio?.exists && !portfolio.error && (
          <>
            <div className="rounded-lg border border-edge p-3">
              <div className="flex flex-wrap items-center gap-4">
                <span className="text-[12px] text-muted">
                  {portfolio.projects.length}{" "}
                  {portfolio.projects.length === 1 ? "project" : "projects"} ·{" "}
                  {summary.trackCount} {summary.trackCount === 1 ? "track" : "tracks"}
                </span>
                {TRACK_COLUMNS.map((col) => (
                  <span key={col.key} className="flex items-center gap-1.5 text-[12px]">
                    <span className={`w-2 h-2 rounded-full ${statusDotClass(col.key)}`} />
                    <span className={statusTextClass(col.key)}>{col.label}</span>
                    <span className="text-muted">{summary.byStatus.get(col.key) ?? 0}</span>
                  </span>
                ))}
              </div>
              <div className="mt-3 flex items-center gap-3">
                <div className="flex-1 h-1.5 rounded-full bg-edge overflow-hidden">
                  <div
                    className="h-full bg-success"
                    style={{ width: `${summary.percentage}%` }}
                  />
                </div>
                <span className="text-[11px] text-muted">
                  {summary.completedTasks}/{summary.totalTasks} tasks ({summary.percentage}%)
                </span>
              </div>
            </div>

            {blockers.length > 0 && (
              <div className="rounded-lg border border-danger/40 p-3">
                <h2 className="text-[13px] text-danger mb-2">
                  Blockers ({blockers.length})
                </h2>
                <ul className="space-y-1">
                  {blockers.map((b, i) => (
                    <li key={`${b.project}-${b.trackId}-${b.phase ?? ""}-${i}`} className="text-[12px]">
                      <span className="text-muted">
                        {b.project} / {b.trackId}
                        {b.phase && ` / ${b.phase}`}:
                      </span>{" "}
                      <span className="text-primary">{b.reason}</span>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {portfolio.projects.length === 0 ? (
              <p className="text-sm text-muted">No projects registered yet.</p>
            ) : (
              <div className="space-y-4">
                {portfolio.projects.map((project) => (
                  <ProjectSection key={project.path} project={project} />
                ))}
              </div>
            )}
          </>
        )}

        {loading && !portfolio && <p className="text-sm text-muted">Loading registry…</p>}
      </div>
    </div>
  );
}
