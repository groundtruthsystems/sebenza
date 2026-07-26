import { useState, type FormEvent } from "react";
import BaseDialog from "./BaseDialog";
import Btn from "./Btn";
import type { UpsertCustomAgentRequest, ValidateCustomAgentResponse } from "./types";
import { errorMessage } from "./utils";

const PLACEHOLDERS = [
  "${PROMPT}",
  "${SYSTEM_PROMPT}",
  "${WORKTREE_PATH}",
  "${REPO_PATH}",
  "${BRANCH}",
  "${PROFILE}",
];

export default function AgentEditorDialog({
  title,
  initialValue,
  onsave,
  onvalidate,
  onclose,
}: {
  title: string;
  initialValue: {
    label: string;
    startCommand: string;
    resumeCommand: string;
  };
  onsave: (value: UpsertCustomAgentRequest) => Promise<void>;
  onvalidate?: (value: UpsertCustomAgentRequest) => Promise<ValidateCustomAgentResponse>;
  onclose: () => void;
}) {
  const [label, setLabel] = useState(initialValue.label);
  const [startCommand, setStartCommand] = useState(initialValue.startCommand);
  const [resumeCommand, setResumeCommand] = useState(initialValue.resumeCommand);
  const [saving, setSaving] = useState(false);
  const [validating, setValidating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [validation, setValidation] = useState<ValidateCustomAgentResponse | null>(null);
  const canSave = label.trim().length > 0 && startCommand.trim().length > 0;

  function buildRequest(): UpsertCustomAgentRequest {
    return {
      label: label.trim(),
      startCommand: startCommand.trim(),
      ...(resumeCommand.trim() ? { resumeCommand: resumeCommand.trim() } : {}),
    };
  }

  async function handleValidate(): Promise<void> {
    if (!canSave || saving || validating || !onvalidate) return;
    setValidating(true);
    setError(null);

    try {
      setValidation(await onvalidate(buildRequest()));
    } catch (err) {
      setError(errorMessage(err));
      setValidation(null);
    } finally {
      setValidating(false);
    }
  }

  async function handleSubmit(): Promise<void> {
    if (!canSave || saving) return;
    setSaving(true);
    setError(null);

    try {
      await onsave(buildRequest());
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <BaseDialog onclose={onclose} wide>
      <form
        onSubmit={(event: FormEvent) => {
          event.preventDefault();
          void handleSubmit();
        }}
      >
        <h2 className="text-base mb-4">{title}</h2>

        <div className="mb-4">
          <label className="block text-xs text-muted mb-1.5" htmlFor="agent-label">
            Agent name
          </label>
          <input
            id="agent-label"
            type="text"
            className="w-full px-2.5 py-1.5 rounded-md border border-edge bg-surface text-primary text-[13px] placeholder:text-muted/50 outline-none focus:border-accent"
            placeholder="e.g. Gemini CLI"
            value={label}
            onChange={(e) => setLabel(e.target.value)}
          />
        </div>

        <div className="mb-4">
          <label className="block text-xs text-muted mb-1.5" htmlFor="agent-start-command">
            Start command
          </label>
          <textarea
            id="agent-start-command"
            rows={4}
            className="w-full px-2.5 py-1.5 rounded-md border border-edge bg-surface text-primary text-[13px] placeholder:text-muted/50 outline-none focus:border-accent resize-y font-mono"
            placeholder={'e.g. pi --append-system-prompt "${SYSTEM_PROMPT}" "${PROMPT}"'}
            value={startCommand}
            onChange={(e) => setStartCommand(e.target.value)}
          ></textarea>
        </div>

        <div className="mb-4">
          <label className="block text-xs text-muted mb-1.5" htmlFor="agent-resume-command">
            Resume command <span className="opacity-60">(optional)</span>
          </label>
          <input
            id="agent-resume-command"
            type="text"
            className="w-full px-2.5 py-1.5 rounded-md border border-edge bg-surface text-primary text-[13px] placeholder:text-muted/50 outline-none focus:border-accent font-mono"
            placeholder={'e.g. pi -c --append-system-prompt "${SYSTEM_PROMPT}"'}
            value={resumeCommand}
            onChange={(e) => setResumeCommand(e.target.value)}
          />
        </div>

        <div className="mb-5 rounded-lg border border-edge bg-surface/40 p-3">
          <p className="text-[13px] text-primary">Supported placeholders</p>
          <div className="mt-2 flex flex-wrap gap-1.5 text-[11px] text-muted font-mono">
            {PLACEHOLDERS.map((placeholder) => (
              <span key={placeholder} className="rounded-full border border-edge px-1.5 py-0.5">
                {placeholder}
              </span>
            ))}
          </div>
          <p className="mt-2 text-[11px] text-muted">
            Sebenza exports placeholder values safely before running your command.
          </p>
        </div>

        {validation && (
          <div className="mb-4 rounded-lg border border-edge bg-surface/40 p-3 text-[12px]">
            <p className="text-primary">
              Agent id: <span className="font-mono">{validation.normalizedId}</span>
            </p>
            {validation.warnings.length > 0 ? (
              <ul className="mt-2 space-y-1 text-muted">
                {validation.warnings.map((warning, i) => (
                  <li key={i}>{warning}</li>
                ))}
              </ul>
            ) : (
              <p className="mt-2 text-success">Configuration looks good.</p>
            )}
          </div>
        )}

        {error && <p className="mb-4 text-[12px] text-danger">{error}</p>}

        <div className="flex justify-end gap-2">
          <Btn type="button" onClick={onclose}>
            Cancel
          </Btn>
          {onvalidate && (
            <Btn
              type="button"
              onClick={() => {
                void handleValidate();
              }}
              disabled={!canSave || validating || saving}
            >
              {validating ? "Testing..." : "Test"}
            </Btn>
          )}
          <Btn type="submit" variant="cta" disabled={!canSave || saving}>
            {saving ? "Saving..." : "Save"}
          </Btn>
        </div>
      </form>
    </BaseDialog>
  );
}
