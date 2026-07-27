import { useState, type ReactNode } from "react";
import type { Track, TrackFileFetcher } from "./types";
import BaseDialog from "./BaseDialog";
import TrackMarkdown from "./TrackMarkdown";

type DocsTab = "spec" | "design";

function TabBtn({
  active,
  disabled,
  onClick,
  children,
}: {
  active: boolean;
  disabled: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={`px-3 py-1 text-[13px] rounded-md border transition-colors disabled:opacity-40 disabled:cursor-not-allowed ${
        active
          ? "border-accent bg-active text-primary"
          : "border-edge text-muted hover:text-primary hover:bg-hover"
      }`}
    >
      {children}
    </button>
  );
}

/** Track "docs" modal — the group's `spec.md` / `design.md` (markdown + mermaid).
 *  The plan is visualized by the board itself + the per-phase detail. */
export default function TrackDetail({
  fetchFile,
  track,
  onclose,
}: {
  fetchFile: TrackFileFetcher;
  track: Track;
  onclose: () => void;
}) {
  const hasSpec = !!track.spec_path;
  const hasDesign = !!track.design_path;
  const [tab, setTab] = useState<DocsTab>(hasSpec ? "spec" : "design");

  return (
    <BaseDialog onclose={onclose} wide maxWidth="90vw">
      <div className="mb-4">
        <div className="flex flex-wrap items-center gap-2">
          {track.type && (
            <span className="text-[10px] uppercase px-1.5 py-0.5 rounded border border-edge text-muted">
              {track.type}
            </span>
          )}
        </div>
        <h2 className="text-base mt-1">{track.description}</h2>
      </div>

      <div className="flex gap-1 mb-3">
        <TabBtn active={tab === "spec"} disabled={!hasSpec} onClick={() => setTab("spec")}>
          Spec
        </TabBtn>
        <TabBtn active={tab === "design"} disabled={!hasDesign} onClick={() => setTab("design")}>
          Design
        </TabBtn>
      </div>

      <div className="overflow-auto max-h-[70vh]">
        {tab === "spec" && hasSpec && <TrackMarkdown fetchFile={fetchFile} path={track.spec_path!} />}
        {tab === "design" && hasDesign && (
          <TrackMarkdown fetchFile={fetchFile} path={track.design_path!} />
        )}
        {tab === "spec" && !hasSpec && (
          <div className="text-sm text-muted py-8 text-center">No spec.md for this track.</div>
        )}
        {tab === "design" && !hasDesign && (
          <div className="text-sm text-muted py-8 text-center">No design.md for this track.</div>
        )}
      </div>
    </BaseDialog>
  );
}
