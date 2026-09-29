import { beforeAll, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import NewDraftDialog from "./NewDraftDialog";

// happy-dom does not implement <dialog>.showModal.
beforeAll(() => {
  if (!HTMLDialogElement.prototype.showModal) {
    HTMLDialogElement.prototype.showModal = function () {
      this.open = true;
    };
    HTMLDialogElement.prototype.close = function () {
      this.open = false;
    };
  }
});

describe("NewDraftDialog", () => {
  it("cannot create without a title", () => {
    render(<NewDraftDialog oncreate={vi.fn()} oncancel={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
  });

  it("treats a whitespace-only title as empty", async () => {
    const user = userEvent.setup();
    render(<NewDraftDialog oncreate={vi.fn()} oncancel={vi.fn()} />);
    await user.type(screen.getByLabelText("Title"), "   ");
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
  });

  it("creates with the trimmed title", async () => {
    const oncreate = vi.fn();
    const user = userEvent.setup();
    render(<NewDraftDialog oncreate={oncreate} oncancel={vi.fn()} />);
    await user.type(screen.getByLabelText("Title"), "  Claims rework  ");
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(oncreate).toHaveBeenCalledWith("Claims rework");
  });

  it("submits on Enter", async () => {
    const oncreate = vi.fn();
    const user = userEvent.setup();
    render(<NewDraftDialog oncreate={oncreate} oncancel={vi.fn()} />);
    await user.type(screen.getByLabelText("Title"), "Quick note{Enter}");
    expect(oncreate).toHaveBeenCalledWith("Quick note");
  });

  it("cancels without creating", async () => {
    const oncreate = vi.fn();
    const oncancel = vi.fn();
    const user = userEvent.setup();
    render(<NewDraftDialog oncreate={oncreate} oncancel={oncancel} />);
    await user.type(screen.getByLabelText("Title"), "Abandoned");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(oncancel).toHaveBeenCalled();
    expect(oncreate).not.toHaveBeenCalled();
  });

  it("shows a failure and keeps the typed title", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <NewDraftDialog oncreate={vi.fn()} oncancel={vi.fn()} />,
    );
    await user.type(screen.getByLabelText("Title"), "Keep me");
    rerender(
      <NewDraftDialog
        error="Unauthorized"
        oncreate={vi.fn()}
        oncancel={vi.fn()}
      />,
    );
    expect(screen.getByText("Unauthorized")).toBeInTheDocument();
    expect(screen.getByLabelText("Title")).toHaveValue("Keep me");
  });

  it("disables the form while creating", () => {
    render(<NewDraftDialog loading oncreate={vi.fn()} oncancel={vi.fn()} />);
    expect(screen.getByLabelText("Title")).toBeDisabled();
    expect(screen.getByRole("button", { name: /Create/ })).toBeDisabled();
  });
});
