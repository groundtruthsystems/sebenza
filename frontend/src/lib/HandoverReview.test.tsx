import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import HandoverReview from "./HandoverReview";
import { useReviewStore } from "./reviewStore";
import { fakeApi } from "./solutionTestApi";
import type { HandoverCard } from "./solutionApi";

const ITEM_ROUTE = "/inbox?item=01ID";
const FULL_ROUTE = "/inbox?item=01ID&handover=acme&view=full";
const here = () => window.location.pathname + window.location.search;

afterEach(cleanup);
beforeEach(() => {
  useReviewStore.setState({ reviews: {} });
  window.history.replaceState(null, "", ITEM_ROUTE);
});

const card = (over: Partial<HandoverCard> = {}): HandoverCard => ({
  project: "/code/acme",
  slug: "acme",
  status: "drafted",
  instructionHash: "ih",
  approvalValid: false,
  copyStale: false,
  rejection: null,
  ...over,
});

function setup(h = card()) {
  const api = fakeApi({
    doc: vi.fn().mockResolvedValue({ name: "handover/acme.md", content: "# Acme\nBuild invoices.", hash: "ih" }),
    rejectHandover: vi.fn().mockResolvedValue({}),
  });
  const onchanged = vi.fn();
  render(<HandoverReview draftId="01ID" handover={h} api={api} designApprovalValid onchanged={onchanged} />);
  return { api, onchanged };
}

const fullScreen = () => screen.queryByRole("region", { name: "Full-screen handover acme" });

describe("HandoverReview", () => {
  // TS-01: a Full screen button opens the handover in a full-screen view.
  it("opens the handover full screen", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Full screen" }));
    const region = fullScreen();
    expect(region).not.toBeNull();
    expect(await within(region!).findByLabelText("Instruction for acme")).toHaveValue("# Acme\nBuild invoices.");
  });

  // TS-02: a visible control leaves full screen.
  it("leaves full screen with the visible Exit control", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Full screen" }));
    await user.click(within(fullScreen()!).getByRole("button", { name: "Exit full screen" }));
    expect(fullScreen()).toBeNull();
  });

  // TS-03 (in context): Esc closes the reject dialog, then full screen.
  it("closes the reject dialog before full screen on Esc", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Full screen" }));
    await user.click(within(fullScreen()!).getByRole("button", { name: "Reject" }));
    expect(screen.getByRole("dialog", { name: "Reject handover acme" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Reject handover acme" })).toBeNull();
    expect(fullScreen()).not.toBeNull();
    await user.keyboard("{Escape}");
    expect(fullScreen()).toBeNull();
  });

  // TS-04: review input and reading position survive mode changes.
  it("keeps the reject comment, instruction edit and scroll across modes", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Review" }));
    const area = await screen.findByLabelText("Instruction for acme");
    await user.clear(area);
    await user.type(area, "edited text");
    await user.click(screen.getByRole("button", { name: "Reject" }));
    await user.type(screen.getByLabelText("Comment"), "wrong team");
    const reader = screen.getByTestId("handover-reader-acme");
    reader.scrollTop = 240;
    reader.dispatchEvent(new Event("scroll"));
    await user.click(screen.getByRole("button", { name: "Full screen" }));
    const fs = fullScreen()!;
    expect(within(fs).getByLabelText("Instruction for acme")).toHaveValue("edited text");
    expect(within(fs).getByLabelText("Comment")).toHaveValue("wrong team");
    expect(within(fs).getByTestId("handover-reader-acme").scrollTop).toBe(240);
    await user.click(within(fs).getByRole("button", { name: "Exit full screen" }));
    expect(screen.getByLabelText("Instruction for acme")).toHaveValue("edited text");
    expect(screen.getByLabelText("Comment")).toHaveValue("wrong team");
    expect(screen.getByTestId("handover-reader-acme").scrollTop).toBe(240);
  });

  // TS-05: the same controls make the same calls in both modes.
  it.each([false, true])("approves, defers and rejects the same way (full screen: %s)", async (full) => {
    const user = userEvent.setup();
    const { api } = setup();
    if (full) await user.click(screen.getByRole("button", { name: "Full screen" }));
    const scope = full ? within(fullScreen()!) : screen;
    await user.click(scope.getByRole("button", { name: "Approve and create worktree" }));
    expect(api.approveHandover).toHaveBeenCalledWith("01ID", "acme", "ih");
    await user.click(scope.getByRole("button", { name: "Defer" }));
    expect(api.deferHandover).toHaveBeenCalledWith("01ID", "acme");
    await user.click(scope.getByRole("button", { name: "Reject" }));
    const dlg = screen.getByRole("dialog", { name: "Reject handover acme" });
    const confirm = within(dlg).getByRole("button", { name: "Confirm reject" });
    expect(confirm).toBeDisabled();
    await user.click(within(dlg).getByRole("radio", { name: "Wrong project" }));
    await user.type(within(dlg).getByLabelText("Comment"), "not affected");
    await user.click(confirm);
    expect(api.rejectHandover).toHaveBeenCalledWith("01ID", "acme", "wrong_project", "not affected");
  });

  it("offers Reject only while the handover awaits review", () => {
    setup(card({ status: "approved" }));
    expect(screen.queryByRole("button", { name: "Reject" })).toBeNull();
  });

  it("shows a rejection with its reason and comment", () => {
    setup(card({ status: "rejected", rejection: { reason: "other", comment: "later", by: "operator_session", at: "2026-11-15T00:00:00Z" } }));
    expect(screen.getByText(/Rejected \(other\): later/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Approve and create worktree" })).toBeNull();
  });

  // TS-08 (fullscreen_request_20261119): full screen is the view=full URL, and
  // Close, Esc and Back each leave it for the prior route.
  it.each(["close", "esc", "back"])("enters at the handover URL and leaves it (%s)", async (how) => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Full screen" }));
    expect(here()).toBe(FULL_ROUTE);
    expect(await within(fullScreen()!).findByLabelText("Instruction for acme")).toBeInTheDocument();
    if (how === "close") await user.click(within(fullScreen()!).getByRole("button", { name: "Exit full screen" }));
    else if (how === "esc") await user.keyboard("{Escape}");
    else act(() => window.history.back());
    await waitFor(() => expect(fullScreen()).toBeNull());
    expect(here()).toBe(ITEM_ROUTE);
    expect(screen.getByRole("button", { name: "Full screen" })).toHaveFocus();
  });

  it("opens full screen straight from its URL, and Close lands on the item", async () => {
    const user = userEvent.setup();
    window.history.replaceState(null, "", FULL_ROUTE);
    setup();
    expect(await within(fullScreen()!).findByLabelText("Instruction for acme")).toBeInTheDocument();
    await user.click(within(fullScreen()!).getByRole("button", { name: "Exit full screen" }));
    expect(fullScreen()).toBeNull();
    expect(here()).toBe(ITEM_ROUTE);
  });

  it("ignores a full-screen URL for another handover", () => {
    window.history.replaceState(null, "", "/inbox?item=01ID&handover=other&view=full");
    setup();
    expect(fullScreen()).toBeNull();
  });

  // TS-09: a nested overlay closes before full screen, which then leaves the URL.
  it("closes the nested reject dialog first, then full screen and its URL", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Full screen" }));
    await user.click(within(fullScreen()!).getByRole("button", { name: "Reject" }));
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Reject handover acme" })).toBeNull();
    expect(fullScreen()).not.toBeNull();
    expect(here()).toBe(FULL_ROUTE);
    await user.keyboard("{Escape}");
    expect(fullScreen()).toBeNull();
    expect(here()).toBe(ITEM_ROUTE);
  });

  // NFR-04: Full screen is keyboard operable and has a visible focus style.
  it("enters full screen from the keyboard", async () => {
    const user = userEvent.setup();
    setup();
    const btn = screen.getByRole("button", { name: "Full screen" });
    expect(btn.className).toMatch(/focus-visible/);
    btn.focus();
    await user.keyboard("{Enter}");
    expect(fullScreen()).not.toBeNull();
  });
});
