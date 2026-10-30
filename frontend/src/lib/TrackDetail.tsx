import { useState, type ReactNode } from "react";
import type { Track, TrackFileFetcher, TrackFileSaver } from "./types";
import BaseDialog from "./BaseDialog";
import Btn from "./Btn";
import ConfirmDialog from "./ConfirmDialog";
import MarkdownEditor from "./MarkdownEditor";
import TrackMarkdown from "./TrackMarkdown";
import { saveDraftBody, type DraftLike } from "./inbox-editor";

type DocsTab = "spec" | "design" | "test-plan";

/** The doc open in the editor: what was loaded (and its hash) and the edits. */
interface Editing {
  path: string;
  loaded: DraftLike;
  body: string;
}

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

/** Track "docs" modal — the group's `spec.md`, `design.md`, and `test-plan.md`
 *  (markdown + mermaid). The plan is visualized by the board itself + the
 *  per-phase detail.
 *
 *  With `saveFile`, each doc can be edited in the inbox's markdown editor and
 *  saved explicitly. The save is gated on the hash the doc was loaded at, so an
 *  agent writing the same file is reported as a conflict, never overwritten. */
export default function TrackDetail({
  fetchFile,
  saveFile,
  track,
  onclose,
}: {
  fetchFile: TrackFileFetcher;
  saveFile?: TrackFileSaver;
  track: Track;
  onclose: () => void;
}) {
  const paths: Record<DocsTab, string | undefined> = {
    spec: track.spec_path,
    design: track.design_path,
    "test-plan": track.test_plan_path,
  };
  const hasSpec = !!paths.spec;
  const hasDesign = !!paths.design;
  const hasTestPlan = !!paths["test-plan"];
  const [tab, setTab] = useState<DocsTab>(hasSpec ? "spec" : hasDesign ? "design" : "test-plan");

  const [editing, setEditing] = useState<Editing | null>(null);
  const [opening, setOpening] = useState(false);
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState("");
  const [conflict, setConflict] = useState<{ theirs: string; theirHash: string } | null>(null);
  /** Bumped after a save so the read-only view re-reads the file. */
  const [version, setVersion] = useState(0);
  /** What to do once the operator agrees to discard unsaved edits. */
  const [pendingDiscard, setPendingDiscard] = useState<(() => void) | null>(null);

  const dirty = !!editing && editing.body !== editing.loaded.body;
  const tabPath = paths[tab];

  /** Run `action`, asking first when it would throw away unsaved edits. */
  const guard = (action: () => void) => {
    if (dirty) setPendingDiscard(() => action);
    else action();
  };

  const stopEditing = () => {
    setEditing(null);
    setConflict(null);
  };

  const switchTab = (next: DocsTab) =>
    guard(() => {
      stopEditing();
      setStatus("");
      setTab(next);
    });

  const startEdit = async () => {
    if (!tabPath) return;
    setOpening(true);
    setStatus("");
    try {
      const res = await fetchFile(tabPath);
      const loaded = { body: res.content, bodyHash: res.hash ?? "" };
      setEditing({ path: tabPath, loaded, body: res.content });
      setConflict(null);
    } catch (err) {
      setStatus((err as Error).message);
    } finally {
      setOpening(false);
    }
  };

  const save = async (base: DraftLike) => {
    if (!editing || !saveFile) return;
    const { path, body } = editing;
    setSaving(true);
    try {
      const outcome = await saveDraftBody(
        {
          save: async (expectedHash, b) => ({
            bodyHash: (await saveFile(path, expectedHash, b)).hash,
          }),
          reload: async () => {
            const res = await fetchFile(path);
            return { body: res.content, bodyHash: res.hash ?? "" };
          },
        },
        base,
        body,
      );
      switch (outcome.kind) {
        case "saved":
        case "unchanged":
          stopEditing();
          setVersion((v) => v + 1);
          setStatus(outcome.kind === "saved" ? "Saved" : "");
          break;
        case "conflict":
          setConflict({ theirs: outcome.theirs, theirHash: outcome.theirHash });
          setStatus("This file changed on disk.");
          break;
        case "error":
          setStatus(outcome.message);
          break;
      }
    } finally {
      setSaving(false);
    }
  };

  const takeTheirs = () => {
    if (!editing || !conflict) return;
    const loaded = { body: conflict.theirs, bodyHash: conflict.theirHash };
    setEditing({ ...editing, loaded, body: conflict.theirs });
    setConflict(null);
    setStatus("Reloaded from disk.");
  };

  const keepMine = () => {
    if (!editing || !conflict) return;
    // Adopt their hash, then write our body over it.
    const loaded = { body: conflict.theirs, bodyHash: conflict.theirHash };
    setEditing({ ...editing, loaded });
    setConflict(null);
    void save(loaded);
  };

  const missing = (
    <div className="text-sm text-muted py-8 text-center">
      No {tab === "test-plan" ? "test-plan.md" : `${tab}.md`} for this track.
    </div>
  );

  return (
    <>
      <BaseDialog onclose={onclose} onrequestclose={() => guard(onclose)} wide maxWidth="90vw">
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

        <div className="flex flex-wrap items-center gap-1 mb-3">
          <TabBtn active={tab === "spec"} disabled={!hasSpec} onClick={() => switchTab("spec")}>
            Spec
          </TabBtn>
          <TabBtn active={tab === "design"} disabled={!hasDesign} onClick={() => switchTab("design")}>
            Design
          </TabBtn>
          <TabBtn
            active={tab === "test-plan"}
            disabled={!hasTestPlan}
            onClick={() => switchTab("test-plan")}
          >
            Test plan
          </TabBtn>
          {saveFile && tabPath && (
            <div className="ml-auto flex items-center gap-2">
              {status && <span className="text-[11px] text-muted">{status}</span>}
              {editing ? (
                <>
                  <Btn onClick={() => guard(stopEditing)} disabled={saving}>
                    Cancel
                  </Btn>
                  <Btn
                    variant="cta"
                    onClick={() => void save(editing.loaded)}
                    disabled={!dirty || saving || !!conflict}
                  >
                    {saving ? "Saving…" : "Save"}
                  </Btn>
                </>
              ) : (
                <Btn variant="accent-outline" onClick={() => void startEdit()} disabled={opening}>
                  Edit
                </Btn>
              )}
            </div>
          )}
        </div>

        {editing && conflict && (
          <div
            className="flex items-center gap-3 px-4 py-2 mb-3 text-xs bg-warning/10 border border-warning rounded-md"
            role="alert"
          >
            <p className="flex-1 m-0">
              This file changed on disk while you were editing. Keep your version, or load theirs?
            </p>
            <Btn small onClick={takeTheirs}>
              Load theirs
            </Btn>
            <Btn small variant="accent-outline" onClick={keepMine}>
              Keep mine
            </Btn>
          </div>
        )}

        {editing ? (
          <div className="h-[70vh] flex flex-col">
            <MarkdownEditor
              value={editing.body}
              onChange={(body) => setEditing((prev) => (prev ? { ...prev, body } : prev))}
              ariaLabel="Track document"
              onNotice={setStatus}
            />
          </div>
        ) : (
          <div className="overflow-auto max-h-[70vh]">
            {tabPath ? (
              <TrackMarkdown fetchFile={fetchFile} path={tabPath} version={version} />
            ) : (
              missing
            )}
          </div>
        )}
      </BaseDialog>
      {pendingDiscard && (
        <ConfirmDialog
          message="Discard unsaved changes?"
          confirmLabel="Discard"
          onconfirm={() => {
            const action = pendingDiscard;
            setPendingDiscard(null);
            action();
          }}
          oncancel={() => setPendingDiscard(null)}
        />
      )}
    </>
  );
}
