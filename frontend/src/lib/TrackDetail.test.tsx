import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import TrackDetail from "./TrackDetail";
import type { Track, TrackFileFetcher } from "./types";

const originalDialogShowModal = HTMLDialogElement.prototype.showModal;
const originalDialogClose = HTMLDialogElement.prototype.close;

const track = {
  track_id: "inbox_20260914",
  type: "feature",
  description: "Markdown inbox",
  status: "doing",
  spec_path: "./tracks/inbox_20260914/spec.md",
  design_path: "./tracks/inbox_20260914/design.md",
  test_plan_path: "./tracks/inbox_20260914/test-plan.md",
  phases_summary: [],
  progress: { total_tasks: 1, completed_tasks: 0, percentage: 0 },
} as Track;

function fetchFileFor(content: Record<string, string>): TrackFileFetcher {
  return vi.fn(async (path: string) => ({
    path,
    content: content[path] ?? "",
  }));
}

describe("TrackDetail", () => {
  beforeEach(() => {
    HTMLDialogElement.prototype.showModal = vi.fn(function (this: HTMLDialogElement): void {
      this.open = true;
    });
    HTMLDialogElement.prototype.close = vi.fn(function (this: HTMLDialogElement): void {
      this.open = false;
    });
  });

  afterEach(() => {
    cleanup();
    HTMLDialogElement.prototype.showModal = originalDialogShowModal;
    HTMLDialogElement.prototype.close = originalDialogClose;
  });

  it("renders test-plan.md beside spec and design", async () => {
    const fetchFile = fetchFileFor({
      "./tracks/inbox_20260914/spec.md": "# Spec\n\nThe requirements.",
      "./tracks/inbox_20260914/test-plan.md": "# Test plan\n\nA login case.",
    });
    const user = userEvent.setup();

    render(<TrackDetail fetchFile={fetchFile} track={track} onclose={() => {}} />);

    await waitFor(() => expect(screen.getByText("The requirements.")).toBeInTheDocument());

    await user.click(screen.getByRole("button", { name: "Test plan" }));

    await waitFor(() => expect(screen.getByText("A login case.")).toBeInTheDocument());
    expect(fetchFile).toHaveBeenCalledWith("./tracks/inbox_20260914/test-plan.md");
  });

  it("disables the test plan tab when the track has no test-plan.md", async () => {
    render(
      <TrackDetail
        fetchFile={fetchFileFor({
          "./tracks/inbox_20260914/spec.md": "# Spec\n\nThe requirements.",
        })}
        track={{ ...track, test_plan_path: undefined }}
        onclose={() => {}}
      />,
    );

    expect(screen.getByRole("button", { name: "Test plan" })).toBeDisabled();
    await waitFor(() => expect(screen.getByText("The requirements.")).toBeInTheDocument());
  });

  it("opens the test plan when it is the only document", async () => {
    const fetchFile = fetchFileFor({
      "./tracks/inbox_20260914/test-plan.md": "A login case.",
    });

    render(
      <TrackDetail
        fetchFile={fetchFile}
        track={{ ...track, spec_path: undefined, design_path: undefined }}
        onclose={() => {}}
      />,
    );

    expect(screen.getByRole("button", { name: "Spec" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Design" })).toBeDisabled();
    await waitFor(() => expect(screen.getByText("A login case.")).toBeInTheDocument());
    expect(fetchFile).toHaveBeenCalledWith("./tracks/inbox_20260914/test-plan.md");
  });
});
