import { describe, expect, it } from "vitest";
import { apiContract, apiPaths } from "./contract";
import {
  ApproveTrackPlanResponseSchema,
  DecideFindingRequestSchema,
  RecordExistingChecksRequestSchema,
  ReviewFindingSchema,
  ReviewJobStatusSchema,
  SignOffTrackResponseSchema,
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
        {
          ruleId: "r",
          path: "a.rs",
          outcome: "clean",
          span: { startLine: 1, endLine: 2 },
        },
      ],
    };
    expect(SubmitReviewRunRequestSchema.safeParse(base).success).toBe(true);
    for (const forged of [
      { ...base, verdict: "passing" },
      { ...base, scope: [] },
      { dispositions: [{ ...base.dispositions[0], status: "resolved" }] },
    ]) {
      expect(SubmitReviewRunRequestSchema.safeParse(forged).success).toBe(
        false,
      );
    }
  });

  it("parses a track with no review job", () => {
    expect(
      TrackReviewStatusSchema.parse({ trackId: "t_1", job: null }).job,
    ).toBeNull();
  });

  it("lists plan approval and parses its boundary snapshot", () => {
    expect(apiPaths.approveTrackPlan).toBe("/api/tracks/:id/plan/approve");
    expect(apiContract.approveTrackPlan.method).toBe("POST");
    const got = ApproveTrackPlanResponseSchema.parse({
      ok: true,
      trackId: "t_1",
      headSha: "h",
      planSha256: "p",
      planContentSha256: "c",
      tasks: [{ taskId: "task-1", files: ["src/"] }],
    });
    expect(got.tasks[0].files).toEqual(["src/"]);
  });

  it("knows the scope-unproven state and boundary findings", () => {
    expect(
      ReviewJobStatusSchema.shape.state.safeParse("scope-unproven").success,
    ).toBe(true);
    expect(ReviewFindingSchema.shape.origin.safeParse("boundary").success).toBe(
      true,
    );
  });

  it("lists the operator decision route and closes its body", () => {
    expect(apiPaths.decideFinding).toBe("/api/findings/:id/decision");
    expect(apiContract.decideFinding.method).toBe("POST");
    for (const decision of ["confirm", "reject"]) {
      expect(DecideFindingRequestSchema.safeParse({ decision }).success).toBe(
        true,
      );
    }
    for (const bad of [
      { decision: "maybe" },
      { decision: "confirm", reason: "x" },
    ]) {
      expect(DecideFindingRequestSchema.safeParse(bad).success).toBe(false);
    }
  });

  it("refuses existing checks in a submission: the skill reports them", () => {
    const withChecks = {
      dispositions: [],
      existingChecks: [{ check: "coverage", status: "not_applicable" }],
    };
    expect(SubmitReviewRunRequestSchema.safeParse(withChecks).success).toBe(
      false,
    );
  });

  it("lists the existing-checks route and closes its body", () => {
    expect(apiPaths.recordExistingChecks).toBe(
      "/api/review/tracks/:id/existing-checks",
    );
    expect(apiContract.recordExistingChecks.method).toBe("POST");
    const base = {
      worktreeId: "wt-1",
      checks: [{ check: "coverage", status: "not_applicable" }],
    };
    expect(RecordExistingChecksRequestSchema.safeParse(base).success).toBe(
      true,
    );
    for (const bad of [
      { check: "lint", status: "pass" },
      { check: "coverage", status: "skipped" },
      { check: "coverage", status: "pass", note: "x" },
    ]) {
      expect(
        RecordExistingChecksRequestSchema.safeParse({ ...base, checks: [bad] })
          .success,
      ).toBe(false);
    }
    expect(
      RecordExistingChecksRequestSchema.safeParse({
        ...base,
        verdict: "passing",
      }).success,
    ).toBe(false);
  });

  it("lists the operator sign-off gate and its refusals", () => {
    expect(apiPaths.signOffTrack).toBe("/api/tracks/:id/signoff");
    expect(apiContract.signOffTrack.method).toBe("POST");
    for (const status of [401, 403, 404, 409, 500] as const) {
      expect(apiContract.signOffTrack.responses[status]).toBeDefined();
    }
    const got = SignOffTrackResponseSchema.parse({
      ok: true,
      appended: true,
      trackId: "t_1",
      jobId: "job_1",
      headSha: "h",
      baseSha: "b",
      findingIds: [],
    });
    expect(got.baseSha).toBe("b");
    expect(
      ReviewJobStatusSchema.shape.signedOff.safeParse(true).success,
    ).toBe(true);
  });
});
