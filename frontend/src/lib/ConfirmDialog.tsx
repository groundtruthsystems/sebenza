import type { FormEvent } from "react";
import BaseDialog from "./BaseDialog";
import Btn from "./Btn";

export default function ConfirmDialog({
  message,
  loading = false,
  error = "",
  confirmLabel = "Remove",
  variant = "danger",
  onconfirm,
  oncancel,
}: {
  message: string;
  loading?: boolean;
  error?: string;
  confirmLabel?: string;
  variant?: "danger" | "accent";
  onconfirm: () => void;
  oncancel: () => void;
}) {
  return (
    <BaseDialog onclose={oncancel}>
      <form
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          onconfirm();
        }}
      >
        <h2 className="text-base mb-4">Confirm</h2>
        <p className="text-[13px] text-muted mb-6">{message}</p>
        {error && (
          <p className="text-[12px] text-danger mb-4 -mt-2 whitespace-pre-wrap">{error}</p>
        )}
        <div className="flex justify-end gap-2">
          <Btn type="button" onClick={oncancel} disabled={loading}>
            Cancel
          </Btn>
          <Btn
            type="submit"
            variant={variant === "accent" ? "cta" : "danger"}
            className="flex items-center gap-1.5"
            disabled={loading}
            autoFocus
          >
            {loading && <span className="spinner"></span>} {confirmLabel}
          </Btn>
        </div>
      </form>
    </BaseDialog>
  );
}
