import { describe, expect, it } from "vitest";
import { apiContract, apiPaths } from "./contract";
import {
  SubmitReviewRunRequestSchema,
  TrackReviewStatusSchema,
} from "./schemas";

// FR-27: the run API and the read-only review routes are in the contract,
// and the submission schema is as closed as the daemon's.
describe("review runs contract", () => {
  it("lists the claim, submit, status and findings routes", () => {
    expect(apiPaths.claimReviewRun).toBe("/api/review-runs/claim");
    expect(apiPaths.submitReviewRun).toBe("/api/review-runs/:id/submit");
    expect(apiPaths.fetchTrackReviewStatus).toBe("/api/review/tracks/:id");
    expect(apiPaths.fetchTrackReviewFindings).toBe(
      "/api/review/tracks/:id/findings",
    );
    expect(apiContract.claimReviewRun.method).toBe("POST");
    expect(apiContract.fetchTrackReviewFindings.method).toBe("GET");
  });

  it("refuses verdict, scope and status fields in a submission", () => {
    const base = {
      dispositions: [
        { ruleId: "r", path: "a.rs", outcome: "clean", span: { startLine: 1, endLine: 2 } },
      ],
    };
    expect(SubmitReviewRunRequestSchema.safeParse(base).success).toBe(true);
    for (const forged of [
      { ...base, verdict: "passing" },
      { ...base, scope: [] },
      { dispositions: [{ ...base.dispositions[0], status: "resolved" }] },
    ]) {
      expect(SubmitReviewRunRequestSchema.safeParse(forged).success).toBe(false);
    }
  });

  it("parses a track with no review job", () => {
    expect(
      TrackReviewStatusSchema.parse({ trackId: "t_1", job: null }).job,
    ).toBeNull();
  });
});
