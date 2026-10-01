import { useState } from "react";
import type { InboxDraftHelpOutput } from "./api-contract";
import { DiffView } from "./InboxRequestCard";
import Btn from "./Btn";
import PhiNotice from "./PhiNotice";

/**
 * Ask the system agent to rework the draft. The proposal is shown, never
 * written: accepting it goes through the editor's own hash-gated save.
 */
export function DraftHelpPanel({
  current,
  pending,
  error,
  proposal,
  onrequest,
  onaccept,
  ondiscard,
  onclose,
}: {
  /** The editor's body, to diff the proposal against. */
  current: string;
  pending: boolean;
  error: string;
  proposal: InboxDraftHelpOutput | null;
  onrequest: (instruction: string) => void;
  onaccept: () => void;
  ondiscard: () => void;
  onclose: () => void;
}) {
  const [instruction, setInstruction] = useState("");

  return (
    <div className="border-b border-edge bg-sidebar px-4 py-2 flex flex-col gap-2 max-h-[45%] overflow-y-auto">
      <form
        className="flex items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (!pending) onrequest(instruction.trim());
        }}
      >
        <input
          aria-label="Draft help instruction"
          placeholder="Optional: what should the agent do with this draft?"
          className="flex-1 min-w-0 h-7 rounded-md border border-edge bg-surface px-2 text-xs text-primary placeholder:text-muted focus:outline-none focus:border-accent"
          value={instruction}
          onChange={(e) => setInstruction(e.currentTarget.value)}
          disabled={pending}
        />
        <Btn type="submit" small variant="accent-outline" disabled={pending}>
          {pending ? "Drafting…" : "Ask agent"}
        </Btn>
        <button
          type="button"
          aria-label="Close draft help"
          className="text-muted hover:text-primary cursor-pointer"
          onClick={onclose}
        >
          &times;
        </button>
      </form>
      <PhiNotice />
      {error && <p className="m-0 text-[11px] text-danger">{error}</p>}
      {proposal && (
        <section aria-label="Draft help proposal" className="flex flex-col gap-2">
          <p className="m-0 text-xs">{proposal.summary}</p>
          <DiffView label="Proposed changes" before={current} after={proposal.proposed_body} />
          <div className="flex justify-end gap-2">
            <Btn small onClick={ondiscard}>
              Discard
            </Btn>
            <Btn small variant="accent-outline" onClick={onaccept}>
              Accept
            </Btn>
          </div>
        </section>
      )}
    </div>
  );
}

/**
 * The body changed under an accepted proposal (a 409). Both versions are
 * shown and nothing is written until the operator saves a resolved body.
 */
export function DraftMergeView({
  theirs,
  theirsLabel,
  mine,
  saving,
  onsave,
  oncancel,
}: {
  theirs: string;
  /** What `theirs` is: on disk, or the editor's unsaved text. */
  theirsLabel: string;
  mine: string;
  saving: boolean;
  onsave: (merged: string) => void;
  oncancel: () => void;
}) {
  const [merged, setMerged] = useState(mine);
  const pane =
    "m-0 flex-1 min-w-0 rounded-md border border-edge bg-surface p-2 text-[11px] font-mono whitespace-pre-wrap overflow-auto max-h-48";

  return (
    <section
      aria-label="Resolve conflict"
      className="border-b border-warning bg-warning/10 px-4 py-2 flex flex-col gap-2 max-h-[60%] overflow-y-auto"
    >
      <p className="m-0 text-xs">
        The draft changed after the proposal was made. Merge the two, then save.
      </p>
      <div className="flex gap-2">
        <div className="flex-1 min-w-0 flex flex-col gap-1">
          <span className="text-[10px] text-muted">{theirsLabel}</span>
          <pre aria-label={theirsLabel} className={pane}>
            {theirs}
          </pre>
        </div>
        <div className="flex-1 min-w-0 flex flex-col gap-1">
          <span className="text-[10px] text-muted">Proposed</span>
          <pre aria-label="Proposed" className={pane}>
            {mine}
          </pre>
        </div>
      </div>
      <textarea
        aria-label="Merged body"
        rows={8}
        className="w-full rounded-md border border-edge bg-surface px-2 py-1.5 text-xs text-primary font-mono focus:outline-none focus:border-accent resize-y"
        value={merged}
        onChange={(e) => setMerged(e.currentTarget.value)}
        disabled={saving}
      />
      <div className="flex flex-wrap justify-end gap-2">
        <Btn small disabled={saving} onClick={() => setMerged(theirs)}>
          Use {theirsLabel.toLowerCase()}
        </Btn>
        <Btn small disabled={saving} onClick={() => setMerged(mine)}>
          Use proposed
        </Btn>
        <Btn small disabled={saving} onClick={oncancel}>
          Cancel
        </Btn>
        <Btn
          small
          variant="accent-outline"
          disabled={saving}
          onClick={() => onsave(merged)}
        >
          Save merged
        </Btn>
      </div>
    </section>
  );
}
