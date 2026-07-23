import { useState, type KeyboardEvent } from "react";
import { formatAskUserQuestionAnswer } from "./ask-user-question";
import type { AskUserQuestionInput } from "./types";

export default function AskUserQuestionCard({
  input,
  disabled,
  onSubmit,
}: {
  input: AskUserQuestionInput;
  disabled: boolean;
  onSubmit: (text: string) => void;
}) {
  // One single-select question can answer on a single tap; anything else
  // (multiple questions, or a multi-select) needs an explicit Submit.
  const autoSend = input.questions.length === 1 && input.questions[0]?.multiSelect !== true;

  const [selections, setSelections] = useState<Record<number, string[]>>({});
  const [customText, setCustomText] = useState<Record<number, string>>({});

  function setSelection(qIndex: number, next: string[]): void {
    setSelections((prev) => ({ ...prev, [qIndex]: next }));
  }

  function setCustom(qIndex: number, value: string): void {
    setCustomText((prev) => ({ ...prev, [qIndex]: value }));
  }

  function isSelected(qIndex: number, label: string): boolean {
    return (selections[qIndex] ?? []).includes(label);
  }

  function buildAnswers(): Array<{ header: string; values: string[] }> {
    return input.questions.map((question, index) => {
      const custom = customText[index]?.trim() ?? "";
      const values = [...(selections[index] ?? [])];
      if (custom.length > 0) values.push(custom);
      return { header: question.header, values };
    });
  }

  const canSubmit = !disabled && buildAnswers().some((answer) => answer.values.length > 0);

  function submitSingle(header: string, value: string): void {
    onSubmit(formatAskUserQuestionAnswer([{ header, values: [value] }]));
  }

  function submitAll(): void {
    if (disabled) return;
    const text = formatAskUserQuestionAnswer(buildAnswers());
    if (text.length === 0) return;
    onSubmit(text);
  }

  function toggleOption(qIndex: number, label: string): void {
    if (disabled) return;
    const question = input.questions[qIndex];
    if (!question) return;
    if (autoSend) {
      submitSingle(question.header, label);
      return;
    }
    const current = selections[qIndex] ?? [];
    if (question.multiSelect) {
      setSelection(
        qIndex,
        current.includes(label) ? current.filter((value) => value !== label) : [...current, label],
      );
    } else {
      setSelection(qIndex, current.includes(label) ? [] : [label]);
    }
  }

  function handleCustomKeydown(event: KeyboardEvent<HTMLInputElement>, qIndex: number): void {
    if (event.key !== "Enter" || event.shiftKey) return;
    event.preventDefault();
    if (disabled) return;
    const custom = customText[qIndex]?.trim() ?? "";
    if (autoSend) {
      const question = input.questions[qIndex];
      if (!question || custom.length === 0) return;
      submitSingle(question.header, custom);
      return;
    }
    if (canSubmit) submitAll();
  }

  return (
    <div className="self-start w-full max-w-[94%] min-w-0 rounded-md border border-accent/40 bg-topbar/40 text-xs text-primary">
      <div className="border-b border-edge/60 px-3 py-2 text-[10px] uppercase tracking-[0.12em] text-muted">
        Question
      </div>

      <div className="flex flex-col gap-4 px-3 py-3">
        {input.questions.map((question, qIndex) => (
          <div key={`${question.header}:${qIndex}`} className="flex min-w-0 flex-col gap-2">
            <div className="text-[10px] uppercase tracking-[0.12em] text-muted">
              {question.header}
            </div>
            <div className="text-sm text-primary">{question.question}</div>
            <div className="flex flex-wrap gap-2">
              {question.options.map((option) => (
                <button
                  key={option.label}
                  type="button"
                  className={`min-w-0 max-w-full rounded-md border px-3 py-1.5 text-left transition disabled:cursor-not-allowed disabled:opacity-60 ${
                    isSelected(qIndex, option.label)
                      ? "border-accent bg-accent text-white"
                      : "border-edge bg-surface text-primary enabled:hover:bg-hover"
                  }`}
                  disabled={disabled}
                  onClick={() => toggleOption(qIndex, option.label)}
                >
                  <span className="block break-words font-medium">{option.label}</span>
                  {option.description && (
                    <span
                      className={`mt-0.5 block break-words text-[10px] ${isSelected(qIndex, option.label) ? "text-white/80" : "text-muted"}`}
                    >
                      {option.description}
                    </span>
                  )}
                </button>
              ))}
            </div>
            <input
              type="text"
              className="w-full rounded-md border border-edge bg-surface px-3 py-1.5 text-xs text-primary outline-none transition placeholder:text-muted/70 focus:border-accent disabled:cursor-not-allowed disabled:opacity-60"
              placeholder="Custom answer…"
              value={customText[qIndex] ?? ""}
              onChange={(event) => setCustom(qIndex, event.currentTarget.value)}
              onKeyDown={(event) => handleCustomKeydown(event, qIndex)}
              disabled={disabled}
            />
          </div>
        ))}
      </div>

      {!autoSend && (
        <div className="flex justify-end border-t border-edge/60 px-3 py-2">
          <button
            type="button"
            className="rounded-md border border-accent bg-accent px-3 py-1.5 text-xs font-medium text-white transition enabled:hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-45"
            onClick={submitAll}
            disabled={!canSubmit}
          >
            Submit answer
          </button>
        </div>
      )}
    </div>
  );
}
