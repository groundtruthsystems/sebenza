import { useEffect, useRef, useState, type FormEvent } from "react";
import BaseDialog from "./BaseDialog";
import Btn from "./Btn";

/**
 * Title prompt for a new inbox draft.
 *
 * The title is frontmatter only — it never reaches the filename, which is a
 * server-issued ULID — so anything typeable here is safe, and it can be
 * renamed later without moving the draft.
 */
export default function NewDraftDialog({
  loading = false,
  error = "",
  oncreate,
  oncancel,
}: {
  loading?: boolean;
  error?: string;
  oncreate: (title: string) => void;
  oncancel: () => void;
}) {
  const [title, setTitle] = useState("");
  const inputEl = useRef<HTMLInputElement>(null);

  const trimmed = title.trim();
  const canCreate = !loading && trimmed.length > 0;

  useEffect(() => {
    const el = inputEl.current;
    if (!el) return;
    queueMicrotask(() => el.focus());
  }, []);

  return (
    <BaseDialog onclose={oncancel}>
      <form
        onSubmit={(event: FormEvent) => {
          event.preventDefault();
          if (canCreate) oncreate(trimmed);
        }}
      >
        <h2 className="text-base mb-4">New draft</h2>
        <div className="mb-4">
          <label
            className="block text-[11px] text-muted mb-1"
            htmlFor="new-draft-title"
          >
            Title
          </label>
          <input
            id="new-draft-title"
            className="w-full px-3 py-2 rounded-md border border-edge bg-surface text-primary text-sm focus:outline-none focus:border-accent"
            maxLength={200}
            ref={inputEl}
            value={title}
            onChange={(event) => setTitle(event.currentTarget.value)}
            placeholder="What is this about?"
            disabled={loading}
          />
          <p className="text-[11px] text-muted mt-1">
            You can rename it later; the file is named by id, not by title.
          </p>
        </div>
        {error && (
          <p className="text-[12px] text-danger mb-4 -mt-2 whitespace-pre-wrap">
            {error}
          </p>
        )}
        <div className="flex justify-end gap-2">
          <Btn type="button" onClick={oncancel} disabled={loading}>
            Cancel
          </Btn>
          <Btn
            type="submit"
            variant="cta"
            className="flex items-center gap-1.5"
            disabled={!canCreate}
          >
            {loading && <span className="spinner"></span>} Create
          </Btn>
        </div>
      </form>
    </BaseDialog>
  );
}
