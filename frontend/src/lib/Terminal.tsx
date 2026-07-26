import { useEffect, useImperativeHandle, useRef, useState, type Ref } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import type { ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { apiBase, uploadFiles } from "./api";
import "@xterm/xterm/css/xterm.css";

export interface TerminalHandle {
  sendSelectPane: (pane: number) => void;
  sendInput: (data: string) => void;
}

const DISCONNECTED_NOTICE = "\r\n\x1b[90m[Disconnected]\x1b[0m";
const RECONNECTED_NOTICE = "\r\n\x1b[32m[Reconnected]\x1b[0m";

export default function Terminal({
  worktree,
  isMobile = false,
  initialPane,
  terminalTheme,
  agentTerminalStale = false,
  refreshingAgentTerminal = false,
  onrefreshagentterminal,
  ref,
}: {
  worktree: string;
  isMobile?: boolean;
  initialPane?: number;
  terminalTheme: ITheme;
  agentTerminalStale?: boolean;
  refreshingAgentTerminal?: boolean;
  onrefreshagentterminal?: () => void;
  ref?: Ref<TerminalHandle>;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<XTerm | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const destroyedRef = useRef(false);
  const canRetryVisibleCloseRef = useRef(true);
  const dragCounterRef = useRef(0);
  const [isDraggingOver, setIsDraggingOver] = useState(false);

  useImperativeHandle(ref, () => ({
    sendSelectPane(pane: number) {
      const ws = wsRef.current;
      if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: "selectPane", pane }));
    },
    sendInput(data: string) {
      const ws = wsRef.current;
      if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: "input", data }));
    },
  }));

  function copyToClipboard(text: string): void {
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(text).catch(() => {});
      return;
    }
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    document.body.removeChild(ta);
  }

  function sendInput(data: string): void {
    const ws = wsRef.current;
    if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: "input", data }));
  }

  async function uploadAndTypeFiles(files: File[]): Promise<void> {
    try {
      const result = await uploadFiles(worktree, files);
      const paths = result.files.map((f) => f.path).join(" ");
      sendInput(paths);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      termRef.current?.writeln(`\r\n\x1b[31m[Upload error: ${msg}]\x1b[0m`);
    }
  }

  function hasDragFiles(dt: DataTransfer | null): boolean {
    if (!dt) return false;
    return dt.types.includes("Files") || dt.types.includes("text/uri-list");
  }

  function handleDragEnter(e: React.DragEvent): void {
    if (!hasDragFiles(e.dataTransfer)) return;
    e.preventDefault();
    dragCounterRef.current++;
    setIsDraggingOver(true);
  }

  function handleDragOver(e: React.DragEvent): void {
    if (!isDraggingOver) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
  }

  function handleDragLeave(): void {
    dragCounterRef.current--;
    if (dragCounterRef.current <= 0) {
      dragCounterRef.current = 0;
      setIsDraggingOver(false);
    }
  }

  function extractImageUrlFromHtml(html: string): string | null {
    const match = html.match(/<img[^>]+src=["']([^"']+)["']/i);
    return match ? match[1] : null;
  }

  async function handleDrop(e: React.DragEvent): Promise<void> {
    e.preventDefault();
    e.stopPropagation();
    dragCounterRef.current = 0;
    setIsDraggingOver(false);

    const dt = e.dataTransfer;
    if (!dt) return;

    let files: File[] = Array.from(dt.files).filter((f) => f.type.startsWith("image/"));

    if (files.length === 0) {
      const html = dt.getData("text/html");
      const uri = dt.getData("text/uri-list");
      const imageUrl = (html ? extractImageUrlFromHtml(html) : null) ?? uri;

      if (imageUrl) {
        try {
          const dataMatch = imageUrl.match(/^data:(image\/[^;]+);base64,(.+)/);
          if (dataMatch) {
            const byteString = atob(dataMatch[2]);
            const bytes = new Uint8Array(byteString.length);
            for (let i = 0; i < byteString.length; i++) bytes[i] = byteString.charCodeAt(i);
            const ext = dataMatch[1].split("/")[1]?.replace("+xml", "") || "png";
            files = [new File([bytes], `image.${ext}`, { type: dataMatch[1] })];
          } else if (/^https?:\/\//i.test(imageUrl)) {
            const resp = await fetch(imageUrl);
            const contentType = resp.headers.get("content-type") ?? "";
            const contentLength = parseInt(resp.headers.get("content-length") ?? "0", 10);
            if (resp.ok && contentType.startsWith("image/") && contentLength <= 10 * 1024 * 1024) {
              const blob = await resp.blob();
              const name = imageUrl.split("/").pop()?.split("?")[0]?.split("#")[0] || "image.png";
              files = [new File([blob], name, { type: blob.type })];
            }
          }
        } catch {
          /* ignore fetch/decode errors for browser drags */
        }
      }
    }

    if (files.length === 0) return;
    await uploadAndTypeFiles(files);
  }

  // Keep the latest theme available to the mount effect without re-running it.
  const themeRef = useRef(terminalTheme);
  themeRef.current = terminalTheme;

  useEffect(() => {
    const containerEl = containerRef.current;
    if (!containerEl) return;
    // Re-enable on every (re)mount — a prior cleanup set destroyedRef true, and
    // a remount (key change, or React re-running the effect) must reconnect.
    destroyedRef.current = false;
    canRetryVisibleCloseRef.current = true;

    let xtermEl: HTMLElement | null = null;
    let viewportEl: HTMLElement | null = null;
    let lastTouchX = 0;
    let lastTouchY = 0;
    let touchScrollLocked = false;
    let manualTouchCleanup: (() => void) | null = null;
    let resizeTimer: ReturnType<typeof setTimeout>;

    const term = new XTerm({
      cursorBlink: true,
      theme: themeRef.current,
      fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', Menlo, monospace",
      fontSize: isMobile ? 13 : 11,
      scrollback: 10000,
    });
    termRef.current = term;

    const fitAddon = new FitAddon();
    fitRef.current = fitAddon;
    term.loadAddon(fitAddon);
    term.loadAddon(new WebLinksAddon());
    term.open(containerEl);

    function shouldUseManualTouchScroll(): boolean {
      return isMobile && !!viewportEl && term.modes.mouseTrackingMode !== "none";
    }
    function handleTouchGestureEnd(): void {
      touchScrollLocked = false;
    }
    function dispatchSyntheticWheel(deltaY: number, touch: Touch): void {
      if (!xtermEl) return;
      const wheelEvent = new WheelEvent("wheel", {
        bubbles: true,
        cancelable: true,
        clientX: touch.clientX,
        clientY: touch.clientY,
        deltaMode: WheelEvent.DOM_DELTA_PIXEL,
        deltaY,
      });
      xtermEl.dispatchEvent(wheelEvent);
    }
    function handleManualTouchStart(event: TouchEvent): void {
      if (!shouldUseManualTouchScroll()) return;
      const touch = event.touches[0];
      if (!touch) return;
      lastTouchX = touch.pageX;
      lastTouchY = touch.pageY;
      touchScrollLocked = false;
    }
    function handleManualTouchMove(event: TouchEvent): void {
      const touch = event.touches[0];
      if (!shouldUseManualTouchScroll() || !viewportEl || !touch) return;
      const deltaX = lastTouchX - touch.pageX;
      const deltaY = lastTouchY - touch.pageY;
      lastTouchX = touch.pageX;
      lastTouchY = touch.pageY;
      if (!touchScrollLocked) {
        if (Math.abs(deltaY) <= Math.abs(deltaX)) return;
        touchScrollLocked = true;
      }
      if (deltaY === 0) return;
      const canScrollViewport = viewportEl.scrollHeight > viewportEl.clientHeight;
      if (!canScrollViewport) {
        dispatchSyntheticWheel(deltaY, touch);
        event.preventDefault();
        return;
      }
      viewportEl.scrollTop += deltaY;
      event.preventDefault();
    }
    function attachManualTouchScroll(root: HTMLElement): void {
      const nextXtermEl = root.querySelector(".xterm");
      const nextViewportEl = root.querySelector(".xterm-viewport");
      if (!(nextXtermEl instanceof HTMLElement) || !(nextViewportEl instanceof HTMLElement)) return;
      xtermEl = nextXtermEl;
      viewportEl = nextViewportEl;
      nextXtermEl.addEventListener("touchstart", handleManualTouchStart, { passive: true });
      nextXtermEl.addEventListener("touchmove", handleManualTouchMove, { passive: false });
      nextXtermEl.addEventListener("touchend", handleTouchGestureEnd);
      nextXtermEl.addEventListener("touchcancel", handleTouchGestureEnd);
      manualTouchCleanup = () => {
        nextXtermEl.removeEventListener("touchstart", handleManualTouchStart);
        nextXtermEl.removeEventListener("touchmove", handleManualTouchMove);
        nextXtermEl.removeEventListener("touchend", handleTouchGestureEnd);
        nextXtermEl.removeEventListener("touchcancel", handleTouchGestureEnd);
        xtermEl = null;
        viewportEl = null;
      };
    }
    attachManualTouchScroll(containerEl);

    function buildResizeMessage(): string {
      const msg = {
        type: "resize" as const,
        cols: term.cols,
        rows: term.rows,
        ...(isMobile && initialPane !== undefined ? { initialPane } : {}),
      };
      return JSON.stringify(msg);
    }

    function connect(announceReconnect = false): void {
      if (
        destroyedRef.current ||
        wsRef.current?.readyState === WebSocket.OPEN ||
        wsRef.current?.readyState === WebSocket.CONNECTING
      ) {
        return;
      }
      const protocol = location.protocol === "https:" ? "wss:" : "ws:";
      const nextWs = new WebSocket(
        `${protocol}//${location.host}${apiBase}/ws/${encodeURIComponent(worktree)}`,
      );
      wsRef.current = nextWs;

      nextWs.onmessage = (event) => {
        const raw = event.data as string;
        const prefix = raw[0];
        if (prefix === "o" || prefix === "s") {
          term.write(raw.slice(1));
          return;
        }
        try {
          const msg = JSON.parse(raw);
          switch (msg.type) {
            case "exit":
              term.writeln(`\r\n\x1b[33m[Process exited with code ${msg.exitCode}]\x1b[0m`);
              break;
            case "error":
              term.writeln(`\r\n\x1b[31m[Error: ${msg.message}]\x1b[0m`);
              break;
          }
        } catch {
          /* Ignore malformed messages */
        }
      };

      nextWs.onerror = () => {};

      nextWs.onopen = () => {
        if (wsRef.current !== nextWs) return;
        canRetryVisibleCloseRef.current = true;
        fitAddon.fit();
        if (announceReconnect) term.writeln(RECONNECTED_NOTICE);
        requestAnimationFrame(() => {
          fitAddon.fit();
          term.focus();
        });
        nextWs.send(buildResizeMessage());
      };

      nextWs.onclose = () => {
        if (wsRef.current !== nextWs) return;
        wsRef.current = null;
        if (destroyedRef.current) return;
        term.writeln(DISCONNECTED_NOTICE);
        if (!document.hidden && canRetryVisibleCloseRef.current) {
          canRetryVisibleCloseRef.current = false;
          connect(true);
        }
      };
    }

    function reconnectIfNeeded(): void {
      if (document.hidden) return;
      connect(true);
    }

    function handlePaste(e: Event): void {
      const clipboard = (e as ClipboardEvent).clipboardData;
      if (!clipboard) return;
      const imageFiles: File[] = [];
      for (const item of clipboard.items) {
        if (item.kind === "file" && item.type.startsWith("image/")) {
          const file = item.getAsFile();
          if (file) imageFiles.push(file);
        }
      }
      if (imageFiles.length === 0) return;
      e.preventDefault();
      e.stopPropagation();
      void uploadAndTypeFiles(imageFiles);
    }

    const preventContextMenu = (e: Event) => e.preventDefault();
    containerEl.addEventListener("contextmenu", preventContextMenu);
    containerEl.addEventListener("paste", handlePaste, true);

    term.parser.registerOscHandler(52, (data) => {
      const idx = data.indexOf(";");
      if (idx !== -1) {
        const b64 = data.slice(idx + 1);
        try {
          copyToClipboard(atob(b64));
        } catch {
          /* ignore */
        }
      }
      return true;
    });

    term.onSelectionChange(() => {
      const sel = term.getSelection();
      if (sel) copyToClipboard(sel);
    });

    term.attachCustomKeyEventHandler((e: KeyboardEvent) => {
      if (e.key === "Enter" && e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
        if (e.type === "keydown" && wsRef.current?.readyState === WebSocket.OPEN) {
          wsRef.current.send(
            JSON.stringify({
              type: "sendKeys",
              hexBytes: ["1b", "5b", "31", "33", "3b", "32", "75"],
            }),
          );
        }
        return false;
      }
      if (e.type !== "keydown") return true;
      const mod = e.metaKey || e.ctrlKey;
      if (mod && (e.key === "c" || e.key === "C")) {
        if (term.hasSelection()) {
          copyToClipboard(term.getSelection());
          term.clearSelection();
          return false;
        }
        return true;
      }
      if (mod && (e.key === "ArrowUp" || e.key === "ArrowDown")) return false;
      if (mod && (e.key === "k" || e.key === "K")) return false;
      if (mod && (e.key === "m" || e.key === "M")) return false;
      if (mod && (e.key === "d" || e.key === "D")) return false;
      return true;
    });

    requestAnimationFrame(() => {
      fitAddon.fit();
      term.focus();
    });

    connect();

    term.onData((data) => {
      if (wsRef.current?.readyState === WebSocket.OPEN) {
        wsRef.current.send(JSON.stringify({ type: "input", data }));
      }
    });

    const resizeObs = new ResizeObserver(() => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        fitAddon.fit();
        if (wsRef.current?.readyState === WebSocket.OPEN) {
          wsRef.current.send(JSON.stringify({ type: "resize", cols: term.cols, rows: term.rows }));
        }
      }, 150);
    });
    resizeObs.observe(containerEl);

    document.addEventListener("visibilitychange", reconnectIfNeeded);
    window.addEventListener("focus", reconnectIfNeeded);
    window.addEventListener("online", reconnectIfNeeded);

    return () => {
      destroyedRef.current = true;
      clearTimeout(resizeTimer);
      manualTouchCleanup?.();
      resizeObs.disconnect();
      containerEl.removeEventListener("contextmenu", preventContextMenu);
      containerEl.removeEventListener("paste", handlePaste, true);
      document.removeEventListener("visibilitychange", reconnectIfNeeded);
      window.removeEventListener("focus", reconnectIfNeeded);
      window.removeEventListener("online", reconnectIfNeeded);
      wsRef.current?.close();
      term.dispose();
      termRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [worktree]);

  // Live theme updates without tearing down the terminal.
  useEffect(() => {
    const term = termRef.current;
    if (term && terminalTheme) term.options.theme = terminalTheme;
  }, [terminalTheme]);

  return (
    <div
      className="flex-1 min-h-0 w-full p-1 overflow-hidden relative"
      ref={containerRef}
      onDragEnter={handleDragEnter}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {agentTerminalStale && (
        <div className="absolute left-3 right-3 top-3 z-20 flex items-center justify-between gap-3 rounded-md border border-warning/40 bg-surface/95 px-4 py-3 text-sm text-primary shadow-lg">
          <span className="min-w-0 truncate">Terminal stale</span>
          {onrefreshagentterminal && (
            <button
              type="button"
              className="shrink-0 rounded-md border border-warning/50 bg-surface px-3 py-1.5 text-xs font-medium text-warning hover:bg-warning/10 disabled:cursor-not-allowed disabled:opacity-50"
              title="Refresh agent terminal"
              onClick={onrefreshagentterminal}
              disabled={refreshingAgentTerminal}
            >
              {refreshingAgentTerminal ? "Refreshing" : "Refresh"}
            </button>
          )}
        </div>
      )}

      {isDraggingOver && (
        <div className="absolute inset-0 z-10 flex items-center justify-center bg-black/50 border-2 border-dashed border-[var(--color-accent)] rounded pointer-events-none">
          <span className="text-white text-sm font-medium">Drop image(s) to upload</span>
        </div>
      )}
    </div>
  );
}
