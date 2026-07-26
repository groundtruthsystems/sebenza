import "./ToastStack.css";
import { useMemo } from "react";
import type { ToastItem, ToastTone } from "./types";
import { deriveToasts, useStore } from "../store";
import { api } from "./api";

function iconForTone(tone: ToastTone): string {
  if (tone === "success") return "✓";
  if (tone === "error") return "✗";
  return "☑";
}

function toneClass(tone: ToastTone): string {
  if (tone === "success") return "text-success";
  if (tone === "error") return "text-danger";
  return "text-accent";
}

function ToastBody({ item }: { item: ToastItem }) {
  return (
    <>
      <span className={`shrink-0 text-base ${toneClass(item.tone)}`}>{iconForTone(item.tone)}</span>
      <span className="flex flex-col gap-0.5 min-w-0">
        <span className="text-sm text-primary whitespace-normal break-words">{item.message}</span>
        {item.detail && (
          <span className="text-xs text-accent whitespace-normal break-all">{item.detail}</span>
        )}
      </span>
    </>
  );
}

export default function ToastStack({ onselect }: { onselect?: (id: string) => void }) {
  const notifications = useStore((s) => s.notifications);
  const uiToasts = useStore((s) => s.uiToasts);
  const toasts = useMemo(() => deriveToasts(notifications, uiToasts), [notifications, uiToasts]);
  const dismissUiToast = useStore((s) => s.dismissUiToast);
  const dismissNotification = useStore((s) => s.dismissNotification);

  if (toasts.length === 0) return null;

  function dismiss(toast: ToastItem): void {
    if (toast.source === "notification") {
      // User-dismiss of a notification: clear locally AND tell the backend
      // (mirrors the old App handleDismissNotification). The SSE-driven removal
      // path in App uses the store directly, so it stays API-free.
      dismissNotification(toast.notificationId);
      api.dismissNotification({ params: { id: toast.notificationId } }).catch(() => {});
      return;
    }
    dismissUiToast(toast.id);
  }

  return (
    <div className="fixed bottom-4 right-4 z-50 flex flex-col items-end gap-2">
      {toasts.map((toast) => (
        <div key={toast.id} className="toast w-fit max-w-[min(48ch,calc(100vw-2rem))]" role="alert">
          {onselect && toast.source === "notification" ? (
            <button
              type="button"
              className="min-w-0 flex items-start gap-2 text-left bg-transparent border-none text-inherit cursor-pointer p-0"
              onClick={() => onselect(toast.id)}
            >
              <ToastBody item={toast} />
            </button>
          ) : (
            <div className="min-w-0 flex items-start gap-2 text-inherit">
              <ToastBody item={toast} />
            </div>
          )}
          <button
            type="button"
            className="shrink-0 w-6 h-6 flex items-center justify-center text-muted hover:text-primary cursor-pointer bg-transparent border-none text-sm"
            onClick={() => dismiss(toast)}
          >
            &times;
          </button>
        </div>
      ))}
    </div>
  );
}
