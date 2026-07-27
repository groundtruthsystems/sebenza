import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import RegistryView from "./RegistryView";
import type { Portfolio } from "./types";

vi.mock("./api", () => ({
  fetchRegistry: vi.fn(),
  fetchRegistryFile: vi.fn(),
}));

import { fetchRegistry } from "./api";

/** One healthy project with a blocked phase, plus one whose workspace is gone —
 *  the two cases the portfolio has to render differently. */
const portfolio = {
  registry_path: "/home/dev/.ai/sebenza/registry.json",
  exists: true,
  error: null,
  projects: [
    {
      name: "alpha",
      path: "/home/dev/alpha",
      tracks_file: "/home/dev/alpha/.ai/sebenza/tracks.json",
      status: "ok",
      error: null,
      tracks: {
        tracks: [
          {
            track_id: "auth_20260727",
            type: "feature",
            description: "Authentication rework",
            status: "doing",
            plan_path: "tracks/auth_20260727/plan.json",
            phases_summary: [
              { id: "phase-1", name: "Schema", status: "done" },
              {
                id: "phase-2",
                name: "Token exchange",
                status: "blocked",
                blocked_reason: "Waiting on IdP credentials",
              },
            ],
            progress: { total_tasks: 10, completed_tasks: 4, percentage: 40 },
          },
        ],
      },
    },
    {
      name: "beta",
      path: "/home/dev/beta",
      tracks_file: "/home/dev/beta/.ai/sebenza/tracks.json",
      status: "missing_tracks",
      error: "No such file or directory (os error 2)",
      tracks: null,
    },
  ],
} as unknown as Portfolio;

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("RegistryView", () => {
  it("rolls up tasks across healthy projects only", async () => {
    vi.mocked(fetchRegistry).mockResolvedValue(portfolio);

    render(<RegistryView />);

    await waitFor(() => expect(screen.getByText("alpha")).toBeInTheDocument());

    // Both projects are listed, but only alpha's single track counts.
    expect(screen.getByText("2 projects · 1 track")).toBeInTheDocument();
    expect(screen.getByText("4/10 tasks (40%)")).toBeInTheDocument();
    expect(screen.getByText("/home/dev/.ai/sebenza/registry.json")).toBeInTheDocument();
  });

  it("surfaces blockers labelled by project and phase", async () => {
    vi.mocked(fetchRegistry).mockResolvedValue(portfolio);

    render(<RegistryView />);

    await waitFor(() => expect(screen.getByText("Blockers (1)")).toBeInTheDocument());
    expect(screen.getByText("alpha / auth_20260727 / Token exchange:")).toBeInTheDocument();
    // Twice: once in the blockers panel, once on the phase card itself.
    expect(screen.getAllByText("Waiting on IdP credentials")).toHaveLength(2);
  });

  it("reports an unhealthy project instead of dropping it", async () => {
    vi.mocked(fetchRegistry).mockResolvedValue(portfolio);

    render(<RegistryView />);

    await waitFor(() => expect(screen.getByText("beta")).toBeInTheDocument());
    expect(screen.getByText("No readable .ai/sebenza/tracks.json")).toBeInTheDocument();
    expect(screen.getByText("No such file or directory (os error 2)")).toBeInTheDocument();
  });

  it("explains an absent registry rather than showing an empty board", async () => {
    vi.mocked(fetchRegistry).mockResolvedValue({
      registry_path: "/home/dev/.ai/sebenza/registry.json",
      exists: false,
      error: null,
      projects: [],
    } as unknown as Portfolio);

    render(<RegistryView />);

    await waitFor(() =>
      expect(screen.getByText(/No registry at/i)).toBeInTheDocument(),
    );
  });

  it("reports a corrupt registry file", async () => {
    vi.mocked(fetchRegistry).mockResolvedValue({
      registry_path: "/home/dev/.ai/sebenza/registry.json",
      exists: true,
      error: "expected value at line 1 column 3",
      projects: [],
    } as unknown as Portfolio);

    render(<RegistryView />);

    await waitFor(() =>
      expect(screen.getByText(/Registry could not be parsed/i)).toBeInTheDocument(),
    );
    expect(screen.getByText("expected value at line 1 column 3")).toBeInTheDocument();
  });
});
