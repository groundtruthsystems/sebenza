// URL-addressable full screen for the inbox (fullscreen_request_20261119).
// Full screen is `view=full` on the request's own route, so it survives a
// reload and can be linked. Entry from inside the app pushes a history entry
// that remembers where it came from; leaving it goes back to that entry.
// After a direct load or a reload there is no in-app entry to go back to, so
// leaving replaces the URL with the request's own route instead.
import { useEffect, useMemo, useRef, useSyncExternalStore } from "react";

/** The inbox route as the URL states it. */
export interface InboxRoute {
  item: string | null;
  /** True only with an item: `view=full` alone means nothing. */
  full: boolean;
  /** One document of the request: `brief`, `requirements` or a note id. */
  doc: string | null;
  /** A handover of a solution item, by slug. */
  handover: string | null;
}

/** Where a full-screen view was opened from, kept in its history entry. */
export interface FullscreenOrigin {
  route: string;
  selection: string | null;
  scroll: number;
}

/** What a full-screen view shows besides the request itself. */
export interface FullscreenTarget {
  doc?: string;
  handover?: string;
}

type OriginProvider = () => Omit<FullscreenOrigin, "route">;

const ROUTE_EVENT = "sebenza:route";
const NO_ORIGIN: OriginProvider = () => ({ selection: null, scroll: 0 });

let originProvider: OriginProvider = NO_ORIGIN;
let opener: { element: Element | null; key: string | null } = { element: null, key: null };
let nextKey = 1;
/** Entries pushed by this page load. A reload forgets them, which is how a
 *  state that survived a reload is told apart from an in-app entry. */
const pushedKeys = new Set<string>();

/** Parse a `location.search` string into the inbox route. */
export function parseInboxRoute(search: string): InboxRoute {
  const q = new URLSearchParams(search);
  const item = q.get("item");
  return {
    item,
    full: !!item && q.get("view") === "full",
    doc: q.get("doc"),
    handover: q.get("handover"),
  };
}

/** The full-screen URL for a request, one of its documents, or a handover. */
export function fullscreenUrl(item: string, target: FullscreenTarget = {}): string {
  const q = new URLSearchParams({ item });
  if (target.handover) q.set("handover", target.handover);
  q.set("view", "full");
  if (target.doc) q.set("doc", target.doc);
  return `/inbox?${q.toString()}`;
}

/** Register what the inbox would need to restore: its selection and list
 *  scroll. `null` unregisters. */
export function setOriginProvider(provider: OriginProvider | null): void {
  originProvider = provider ?? NO_ORIGIN;
}

function notify(): void {
  window.dispatchEvent(new Event(ROUTE_EVENT));
}

/** Open full screen from inside the app, remembering the origin and the
 *  control that opened it. */
export function openFullscreen(item: string, target: FullscreenTarget = {}): void {
  const active = document.activeElement;
  opener = { element: active, key: active?.getAttribute("data-fullscreen-opener") ?? null };
  const origin: FullscreenOrigin = {
    route: window.location.pathname + window.location.search,
    ...originProvider(),
  };
  const fullscreenKey = String(nextKey++);
  pushedKeys.add(fullscreenKey);
  window.history.pushState({ fullscreen: origin, fullscreenKey }, "", fullscreenUrl(item, target));
  notify();
}

/** The origin of the current entry, when this page load pushed it. */
export function inAppOrigin(): FullscreenOrigin | null {
  const state: unknown = window.history.state;
  if (!state || typeof state !== "object") return null;
  const { fullscreen, fullscreenKey } = state as { fullscreen?: FullscreenOrigin; fullscreenKey?: string };
  return fullscreen && fullscreenKey && pushedKeys.has(fullscreenKey) ? fullscreen : null;
}

/** Leave full screen: back to the origin after an in-app entry, otherwise to
 *  the request's own route. */
export function closeFullscreen(item: string): void {
  if (inAppOrigin()) {
    window.history.back();
    return;
  }
  window.history.replaceState(null, "", `/inbox?item=${encodeURIComponent(item)}`);
  notify();
}

/** Return focus to the control that opened full screen. A control that was
 *  re-rendered meanwhile is found again by its `data-fullscreen-opener`. */
export function focusOpener(): void {
  const { element, key } = opener;
  const target =
    element?.isConnected && element instanceof HTMLElement
      ? element
      : key
        ? document.querySelector<HTMLElement>(`[data-fullscreen-opener="${key}"]`)
        : null;
  target?.focus();
}

function subscribe(onChange: () => void): () => void {
  window.addEventListener("popstate", onChange);
  window.addEventListener(ROUTE_EVENT, onChange);
  return () => {
    window.removeEventListener("popstate", onChange);
    window.removeEventListener(ROUTE_EVENT, onChange);
  };
}

/** The inbox route, re-read on Back/Forward and on every in-app change. */
export function useInboxRoute(): InboxRoute {
  const search = useSyncExternalStore(subscribe, () => window.location.search);
  return useMemo(() => parseInboxRoute(search), [search]);
}

/** How the inbox puts things back when full screen closes. */
export interface FullscreenReturn {
  /** After an in-app entry: the saved route is already back in the URL. */
  restore: (origin: FullscreenOrigin) => void;
  /** After a direct load or reload: show this request, selected and in view. */
  reveal: (item: string | null) => void;
}

/** Call `restore` or `reveal` whenever the route leaves full screen, however
 *  it left: Close, Esc or browser Back. */
export function useFullscreenReturn(route: InboxRoute, on: FullscreenReturn): void {
  const latest = useRef(on);
  latest.current = on;
  const origin = useRef<FullscreenOrigin | null>(null);
  const wasFull = useRef(false);
  useEffect(() => {
    if (route.full) {
      origin.current = inAppOrigin();
      wasFull.current = true;
      return;
    }
    if (!wasFull.current) return;
    wasFull.current = false;
    if (origin.current) latest.current.restore(origin.current);
    else latest.current.reveal(route.item);
  }, [route.full, route.item]);
}
