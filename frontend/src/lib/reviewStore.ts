// Review state for one handover, shared by the normal and the full-screen
// view (FR-15). Keyed `<draftId>:<slug>`, so entering or leaving full screen
// (which remounts the reader) never loses the instruction edit, the reject
// form or the reading position.
import { create } from "zustand";

export type RejectReason = "wrong_project" | "other";

export interface HandoverReviewState {
  /** The loaded instruction and its hash, once Review was pressed. */
  doc: { content: string; hash: string } | null;
  /** The operator's unsaved edit of the instruction. */
  draft: string | null;
  rejectOpen: boolean;
  reason: RejectReason;
  comment: string;
  /** The reader's scroll offset, restored in either mode. */
  scrollTop: number;
  fullscreen: boolean;
  chatOpen: boolean;
}

export const EMPTY_REVIEW: HandoverReviewState = {
  doc: null,
  draft: null,
  rejectOpen: false,
  reason: "other",
  comment: "",
  scrollTop: 0,
  fullscreen: false,
  chatOpen: false,
};

interface ReviewStore {
  reviews: Record<string, HandoverReviewState>;
  patch: (key: string, change: Partial<HandoverReviewState>) => void;
  reset: (key: string) => void;
}

export const useReviewStore = create<ReviewStore>((set) => ({
  reviews: {},
  patch: (key, change) =>
    set((s) => ({
      reviews: { ...s.reviews, [key]: { ...(s.reviews[key] ?? EMPTY_REVIEW), ...change } },
    })),
  reset: (key) =>
    set((s) => {
      const next = { ...s.reviews };
      delete next[key];
      return { reviews: next };
    }),
}));

export const reviewKey = (draftId: string, slug: string) => `${draftId}:${slug}`;

/** The review state for one handover and a setter bound to it. */
export function useHandoverReview(draftId: string, slug: string) {
  const key = reviewKey(draftId, slug);
  const state = useReviewStore((s) => s.reviews[key]) ?? EMPTY_REVIEW;
  const patch = useReviewStore((s) => s.patch);
  return [state, (change: Partial<HandoverReviewState>) => patch(key, change)] as const;
}
