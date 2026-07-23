import { useEffect, useRef, useState, type FormEvent } from "react";
import BaseDialog from "./BaseDialog";
import Btn from "./Btn";

export default function WorktreeLabelDialog({
  branch,
  initialLabel,
  loading = false,
  error = "",
  onconfirm,
  onclear,
  oncancel,
}: {
  branch: string;
  initialLabel: string | null;
  loading?: boolean;
  error?: string;
  onconfirm: (label: string) => void;
  onclear: () => void;
  oncancel: () => void;
}) {
  const [currentLabel, setCurrentLabel] = useState<string | null>(null);
  const inputEl = useRef<HTMLInputElement>(null);

  const normalizedInitialLabel = (initialLabel ?? "").trim();
  const normalizedLabel = (currentLabel ?? initialLabel ?? "").trim();
  const canSave = !loading && normalizedLabel !== normalizedInitialLabel;

  useEffect(() => {
    const currentInput = inputEl.current;
    if (!currentInput) return;
    queueMicrotask(() => currentInput.focus());
  }, []);

  return (
    <BaseDialog onclose={oncancel}>
      <form
        onSubmit={(event: FormEvent) => {
          event.preventDefault();
          if (canSave) onconfirm(normalizedLabel);
        }}
      >
        <h2 className="text-base mb-4">Workspace label</h2>
        <div className="mb-4">
          <label className="block text-[11px] text-muted mb-1" htmlFor="worktree-label-input">
            Label
          </label>
          <input
            id="worktree-label-input"
            className="w-full px-3 py-2 rounded-md border border-edge bg-surface text-primary text-sm focus:outline-none focus:border-accent"
            maxLength={80}
            ref={inputEl}
            value={currentLabel ?? initialLabel ?? ""}
            onChange={(event) => setCurrentLabel(event.currentTarget.value)}
            placeholder={branch}
            disabled={loading}
          />
        </div>
        {error && (
          <p className="text-[12px] text-danger mb-4 -mt-2 whitespace-pre-wrap">{error}</p>
        )}
        <div className="flex justify-between gap-2">
          <Btn type="button" onClick={onclear} disabled={loading || !initialLabel}>
            Clear
          </Btn>
          <div className="flex justify-end gap-2">
            <Btn type="button" onClick={oncancel} disabled={loading}>
              Cancel
            </Btn>
            <Btn type="submit" variant="cta" className="flex items-center gap-1.5" disabled={!canSave}>
              {loading && <span className="spinner"></span>} Save
            </Btn>
          </div>
        </div>
      </form>
    </BaseDialog>
  );
}
