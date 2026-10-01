import { describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import InboxComments from "./InboxComments";
import type { InboxComment, InboxCommentGroups } from "./api-contract";

const comment = (over: Partial<InboxComment>): InboxComment => ({
  eventId: "E1",
  ts: "2026-09-30T10:00:00Z",
  author: "operator",
  caller: null,
  kind: "note",
  body: "hello",
  title: null,
  requestId: null,
  parentEventId: null,
  warnings: [],
  redacted: false,
  ...over,
});

const wt = { project: "/code/acme", branch: "fix-x" };

function groups(): InboxCommentGroups {
  return {
    overall: [
      comment({
        eventId: "E1",
        body: "**plan** <script>window.__pwned = 1</script>",
      }),
      comment({
        eventId: "E2",
        author: "system_agent",
        kind: "advice",
        body: "Consider splitting the loader.",
      }),
    ],
    worktrees: [
      {
        ...wt,
        comments: [
          comment({
            eventId: "E3",
            author: "worktree_agent",
            caller: "claude@fix-x",
            body: '<img src=x onerror="alert(1)"> [docs](javascript:alert(2)) progress',
          }),
          comment({
            eventId: "E4",
            author: "worktree_agent",
            body: "ssn 123-45-6789",
            warnings: ["likely PHI: SSN"],
          }),
          comment({
            eventId: "E5",
            author: "worktree_agent",
            body: "[redacted]",
            redacted: true,
          }),
        ],
      },
    ],
  };
}

function setup(over: Partial<Parameters<typeof InboxComments>[0]> = {}) {
  const onpost = vi.fn().mockResolvedValue(comment({ eventId: "E9" }));
  const onredact = vi.fn().mockResolvedValue(undefined);
  const utils = render(
    <InboxComments
      groups={groups()}
      worktrees={[wt, { project: "/code/beta", branch: "spike" }]}
      onpost={onpost}
      onredact={onredact}
      {...over}
    />,
  );
  return { ...utils, onpost, onredact };
}

describe("InboxComments", () => {
  // TS-09: all three authors, in the right group, markup inert.
  it("groups comments per worktree plus overall, labelled by author", () => {
    setup();
    const overall = screen.getByRole("region", { name: "Overall thread" });
    const worktree = screen.getByRole("region", { name: "acme · fix-x thread" });
    expect(within(overall).getByText("plan")).toBeInTheDocument();
    expect(within(overall).getByText("operator")).toBeInTheDocument();
    expect(within(overall).getByText("system agent")).toBeInTheDocument();
    expect(within(worktree).getAllByText("worktree agent").length).toBeGreaterThan(0);
    expect(within(worktree).getByText(/progress/)).toBeInTheDocument();
    expect(within(overall).queryByText(/progress/)).toBeNull();
  });

  // TS-09 / TS-43: nothing executable reaches the DOM.
  it("renders bodies as sanitised markdown only", () => {
    const { container } = setup();
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("[onerror]")).toBeNull();
    expect(container.querySelector('a[href^="javascript"]')).toBeNull();
    expect(container.querySelector("strong")?.textContent).toBe("plan");
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  it("marks advice as never delivered", () => {
    setup();
    expect(screen.getByText(/not delivered/i)).toBeInTheDocument();
  });

  it("shows a self-declared caller as a claim", () => {
    setup();
    expect(screen.getByText(/claude@fix-x/)).toBeInTheDocument();
  });

  // TS-66: a likely-PHI string is stored unchanged and badged.
  it("badges scan hits", () => {
    setup();
    expect(screen.getByText("likely PHI: SSN")).toBeInTheDocument();
  });

  it("masks a redacted body and offers no second redaction", () => {
    setup();
    const worktree = screen.getByRole("region", { name: "acme · fix-x thread" });
    const masked = within(worktree).getByText("Redacted");
    const row = masked.closest("li")!;
    expect(within(row).queryByRole("button", { name: /redact/i })).toBeNull();
    expect(within(row).queryByText("[redacted]")).toBeNull();
  });

  it("redacts only after confirming", async () => {
    const user = userEvent.setup();
    const { onredact } = setup();
    const overall = screen.getByRole("region", { name: "Overall thread" });
    const first = within(overall).getAllByRole("listitem")[0];
    await user.click(within(first).getByRole("button", { name: "Redact" }));
    expect(onredact).not.toHaveBeenCalled();
    await user.click(within(first).getByRole("button", { name: "Confirm redact" }));
    expect(onredact).toHaveBeenCalledWith("E1");
  });

  // TS-66: the composer says PHI is prohibited and where content goes.
  it("warns about PHI and the model provider in the composer", () => {
    setup();
    const note = screen.getByRole("note");
    expect(note).toHaveTextContent(/PHI/);
    expect(note).toHaveTextContent(/model provider/i);
  });

  it("posts to the overall thread by default", async () => {
    const user = userEvent.setup();
    const { onpost } = setup();
    await user.type(screen.getByLabelText("Comment"), "  ship it  ");
    await user.click(screen.getByRole("button", { name: "Post comment" }));
    expect(onpost).toHaveBeenCalledWith("ship it", undefined);
    await waitFor(() => expect(screen.getByLabelText("Comment")).toHaveValue(""));
  });

  it("posts to a chosen worktree, including one with no comments yet", async () => {
    const user = userEvent.setup();
    const { onpost } = setup();
    await user.selectOptions(screen.getByLabelText("Comment thread"), "beta · spike");
    await user.type(screen.getByLabelText("Comment"), "look here");
    await user.click(screen.getByRole("button", { name: "Post comment" }));
    expect(onpost).toHaveBeenCalledWith("look here", {
      project: "/code/beta",
      branch: "spike",
    });
  });

  it("says when a posted comment was flagged by the scan", async () => {
    const user = userEvent.setup();
    setup({
      onpost: vi
        .fn()
        .mockResolvedValue(comment({ warnings: ["likely PHI: date of birth"] })),
    });
    await user.type(screen.getByLabelText("Comment"), "dob 1980-01-01");
    await user.click(screen.getByRole("button", { name: "Post comment" }));
    expect(
      await screen.findByText(/flagged.*likely PHI: date of birth/i),
    ).toBeInTheDocument();
  });

  it("keeps the text when posting fails", async () => {
    const user = userEvent.setup();
    setup({ onpost: vi.fn().mockRejectedValue(new Error("rate limited")) });
    await user.type(screen.getByLabelText("Comment"), "keep me");
    await user.click(screen.getByRole("button", { name: "Post comment" }));
    expect(await screen.findByText("rate limited")).toBeInTheDocument();
    expect(screen.getByLabelText("Comment")).toHaveValue("keep me");
  });
});
