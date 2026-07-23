import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ConductorBoard from "./ConductorBoard";
import type { ConductorTracks, WorktreeInfo } from "./types";

vi.mock("./api", () => ({
  fetchConductorTracks: vi.fn(),
  fetchConductorFile: vi.fn(),
}));

import { fetchConductorTracks } from "./api";

const worktree = { branch: "feature/x" } as unknown as WorktreeInfo;

const sampleTracks = {
  tracks: [
    {
      track_id: "interactive_gateway_20260720",
      type: "feature",
      description: "Agent service accounts + gateway integration",
      status: "doing",
      phases_summary: [{ id: "phase-1", name: "Foundation", status: "done" }],
      progress: { total_tasks: 16, completed_tasks: 15, percentage: 94 },
    },
  ],
} as unknown as ConductorTracks;

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("ConductorBoard", () => {
  it("renders status columns and places a track card in its column with progress", async () => {
    vi.mocked(fetchConductorTracks).mockResolvedValue(sampleTracks);

    render(<ConductorBoard worktree={worktree} />);

    await waitFor(() =>
      expect(
        screen.getByText("Agent service accounts + gateway integration"),
      ).toBeInTheDocument(),
    );

    // All five kanban columns render.
    for (const col of ["Backlog", "Doing", "Blocked", "Unblocked", "Done"]) {
      expect(screen.getByText(col)).toBeInTheDocument();
    }
    // Card shows progress and a phase summary.
    expect(screen.getByText("15/16 (94%)")).toBeInTheDocument();
    expect(screen.getByText("Foundation")).toBeInTheDocument();
  });

  it("shows the empty state when there is no conductor board", async () => {
    vi.mocked(fetchConductorTracks).mockResolvedValue(null);

    render(<ConductorBoard worktree={worktree} />);

    await waitFor(() =>
      expect(screen.getByText(/No conductor tracks for this worktree/i)).toBeInTheDocument(),
    );
  });
});
