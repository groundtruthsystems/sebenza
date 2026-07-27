import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import TracksBoard from "./TracksBoard";
import type { Tracks, WorktreeInfo } from "./types";

vi.mock("./api", () => ({
  fetchTracks: vi.fn(),
  fetchTrackFile: vi.fn(),
}));

import { fetchTracks } from "./api";

const worktree = { branch: "feature/x" } as unknown as WorktreeInfo;

const sampleTracks = {
  tracks: [
    {
      track_id: "interactive_gateway_20260720",
      type: "feature",
      description: "Agent service accounts + gateway integration",
      status: "doing",
      plan_path: "./tracks/interactive_gateway_20260720/plan.json",
      spec_path: "./tracks/interactive_gateway_20260720/spec.md",
      design_path: "./tracks/interactive_gateway_20260720/design.md",
      phases_summary: [
        { id: "phase-1", name: "Foundation", status: "done" },
        { id: "phase-6", name: "Integration", status: "doing" },
      ],
      progress: { total_tasks: 16, completed_tasks: 15, percentage: 94 },
    },
  ],
} as unknown as Tracks;

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("TracksBoard", () => {
  it("renders a track group with its phases as cards under status columns", async () => {
    vi.mocked(fetchTracks).mockResolvedValue(sampleTracks);

    render(<TracksBoard worktree={worktree} />);

    // Group header shows the track description + phase completion.
    await waitFor(() =>
      expect(
        screen.getByText("Agent service accounts + gateway integration"),
      ).toBeInTheDocument(),
    );
    expect(screen.getByText("1/2 phases")).toBeInTheDocument();

    // Status columns render, and phases appear as cards.
    for (const col of ["Backlog", "Doing", "Blocked", "Unblocked", "Done"]) {
      expect(screen.getByText(col)).toBeInTheDocument();
    }
    expect(screen.getByText("Foundation")).toBeInTheDocument();
    expect(screen.getByText("Integration")).toBeInTheDocument();

    // A View action is offered for the group's spec/design docs.
    expect(screen.getByRole("button", { name: "View" })).toBeInTheDocument();
  });

  it("shows the empty state when there is no Sebenza workspace", async () => {
    vi.mocked(fetchTracks).mockResolvedValue(null);

    render(<TracksBoard worktree={worktree} />);

    await waitFor(() =>
      expect(screen.getByText(/No Sebenza tracks for this worktree/i)).toBeInTheDocument(),
    );
  });
});
