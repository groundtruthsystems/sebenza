// The layered Escape manager (FR-16): Esc closes the innermost open layer
// (dialog, then chat, then full screen), one layer per press. One document
// listener serves every layer, so a press never closes two things at once.
// A native <dialog> open with showModal handles its own Escape (BaseDialog's
// cancel), so while one is open this manager stays out of the way.
import { useEffect, useRef } from "react";

export type EscLayerKind = "dialog" | "chat" | "fullscreen";

const PRIORITY: Record<EscLayerKind, number> = { dialog: 3, chat: 2, fullscreen: 1 };

interface Layer {
  id: number;
  kind: EscLayerKind;
  close: () => void;
}

const layers: Layer[] = [];
let nextId = 1;
let listening = false;

function onKeyDown(e: KeyboardEvent) {
  if (e.key !== "Escape" || layers.length === 0) return;
  if (document.querySelector("dialog[open]")) return;
  // Highest kind wins; within a kind, the most recently opened.
  const top = [...layers].sort((a, b) => PRIORITY[b.kind] - PRIORITY[a.kind] || b.id - a.id)[0];
  e.preventDefault();
  e.stopPropagation();
  top.close();
}

function ensureListening() {
  if (listening) return;
  document.addEventListener("keydown", onKeyDown);
  listening = true;
}

/** Register a closable layer while `active`. `onClose` may change identity
 *  between renders; the latest one is called. */
export function useEscLayer(kind: EscLayerKind, active: boolean, onClose: () => void) {
  const latest = useRef(onClose);
  latest.current = onClose;
  useEffect(() => {
    if (!active) return;
    ensureListening();
    const layer: Layer = { id: nextId++, kind, close: () => latest.current() };
    layers.push(layer);
    return () => {
      const i = layers.indexOf(layer);
      if (i >= 0) layers.splice(i, 1);
    };
  }, [kind, active]);
}
