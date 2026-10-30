import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import TrackDetail from "./TrackDetail";
import type { Track, TrackFileFetcher, TrackFileSaver } from "./types";

// Mermaid pulls a large async graph that has no place in an editing test.
vi.mock("./inboxMarkdown", () => ({
  renderDraftMarkdown: (s: string) => Promise.resolve(`<p>${s}</p>`),
}));

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

  describe("editing", () => {
    const SPEC = "./tracks/inbox_20260914/spec.md";

    /** A disk with one spec whose hash is its revision number. */
    function disk(initial: string) {
      const file = { content: initial, rev: 1 };
      const fetchFile: TrackFileFetcher = vi.fn(async (path: string) => ({
        path,
        content: file.content,
        hash: `h${file.rev}`,
      }));
      const saveFile = vi.fn<TrackFileSaver>(async (_path, expectedHash, content) => {
        if (expectedHash !== `h${file.rev}`) {
          throw new Error(`Track file conflict: ${SPEC} changed on disk`);
        }
        file.content = content;
        file.rev += 1;
        return { hash: `h${file.rev}` };
      });
      /** Someone else (an agent) writes the file. */
      const external = (content: string) => {
        file.content = content;
        file.rev += 1;
      };
      return { file, fetchFile, saveFile, external };
    }

    it("is read-only without a saver", async () => {
      const { fetchFile } = disk("The requirements.");
      render(<TrackDetail fetchFile={fetchFile} track={track} onclose={() => {}} />);
      await waitFor(() => expect(screen.getByText("The requirements.")).toBeInTheDocument());
      expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
    });

    it("saves against the loaded hash and shows the saved doc", async () => {
      const { file, fetchFile, saveFile } = disk("old spec");
      const user = userEvent.setup();
      render(<TrackDetail fetchFile={fetchFile} saveFile={saveFile} track={track} onclose={() => {}} />);

      await user.click(await screen.findByRole("button", { name: "Edit" }));
      const editor = await screen.findByLabelText("Track document");
      expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
      await user.clear(editor);
      await user.type(editor, "new spec");
      await user.click(screen.getByRole("button", { name: "Save" }));

      await waitFor(() => expect(saveFile).toHaveBeenCalledWith(SPEC, "h1", "new spec"));
      expect(file.content).toBe("new spec");
      await waitFor(() => expect(screen.getByText("new spec")).toBeInTheDocument());
      expect(screen.queryByLabelText("Track document")).not.toBeInTheDocument();
      expect(screen.getByText("Saved")).toBeInTheDocument();
    });

    it("reports a file that moved on disk, and keeps mine over theirs on request", async () => {
      const { file, fetchFile, saveFile, external } = disk("old spec");
      const user = userEvent.setup();
      render(<TrackDetail fetchFile={fetchFile} saveFile={saveFile} track={track} onclose={() => {}} />);

      await user.click(await screen.findByRole("button", { name: "Edit" }));
      const editor = await screen.findByLabelText("Track document");
      await user.clear(editor);
      await user.type(editor, "mine");
      external("theirs");
      await user.click(screen.getByRole("button", { name: "Save" }));

      expect(await screen.findByRole("alert")).toHaveTextContent(/changed on disk/);
      expect(file.content).toBe("theirs");

      await user.click(screen.getByRole("button", { name: "Keep mine" }));
      await waitFor(() => expect(saveFile).toHaveBeenLastCalledWith(SPEC, "h2", "mine"));
      expect(file.content).toBe("mine");
    });

    it("loads theirs into the editor on request", async () => {
      const { fetchFile, saveFile, external } = disk("old spec");
      const user = userEvent.setup();
      render(<TrackDetail fetchFile={fetchFile} saveFile={saveFile} track={track} onclose={() => {}} />);

      await user.click(await screen.findByRole("button", { name: "Edit" }));
      const editor = await screen.findByLabelText("Track document");
      await user.type(editor, " plus mine");
      external("theirs");
      await user.click(screen.getByRole("button", { name: "Save" }));
      await user.click(await screen.findByRole("button", { name: "Load theirs" }));

      expect(screen.getByLabelText("Track document")).toHaveValue("theirs");
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    });

    it("asks before discarding unsaved edits", async () => {
      const { fetchFile, saveFile } = disk("old spec");
      const onclose = vi.fn();
      const user = userEvent.setup();
      render(<TrackDetail fetchFile={fetchFile} saveFile={saveFile} track={track} onclose={onclose} />);

      await user.click(await screen.findByRole("button", { name: "Edit" }));
      await user.type(await screen.findByLabelText("Track document"), " edited");

      // Switching tabs is guarded.
      await user.click(screen.getByRole("button", { name: "Design" }));
      expect(await screen.findByText("Discard unsaved changes?")).toBeInTheDocument();
      expect(screen.getByLabelText("Track document")).toBeInTheDocument();
      await user.click(screen.getAllByRole("button", { name: "Cancel" }).at(-1)!);
      expect(screen.queryByText("Discard unsaved changes?")).not.toBeInTheDocument();

      // Cancel, confirmed, drops the edits and nothing is saved.
      await user.click(screen.getByRole("button", { name: "Cancel" }));
      await user.click(await screen.findByRole("button", { name: "Discard" }));
      await waitFor(() => expect(screen.getByText("old spec")).toBeInTheDocument());
      expect(screen.queryByLabelText("Track document")).not.toBeInTheDocument();
      expect(saveFile).not.toHaveBeenCalled();
      expect(onclose).not.toHaveBeenCalled();
    });
  });
});
