import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { InboxRequest } from "./api-contract";

const req = (requestId: string, status: InboxRequest["status"], over: Partial<InboxRequest> = {}): InboxRequest => ({
  requestId,
  worktree: { project: "/code/acme", branch: "fix-x" },
  title: `Title ${requestId}`,
  body: "body",
  status,
  flagged: false,
  proposalId: null,
  proposal: null,
  proposalHash: null,
  proposalWarnings: [],
  contentHash: null,
  confirmedText: null,
  lastReason: null,
  lastError: null,
  attempts: 0,
  warnings: [],
  openedAt: "2026-09-30T10:00:00Z",
  archivedAt: null,
  archivedBy: null,
  dismissedAt: null,
  dismissedBy: null,
  summary: { request: `fix-x: Title ${requestId}`, result: null },
  notice: null,
  ...over,
});

// The server applies the filter; this fake applies the same mapping (FR-05).
const ALL = [
  req("R-open", "open"),
  req("R-failed", "delivery_failed"),
  req("R-resolved", "resolved", { archivedAt: "2026-10-01T00:00:00Z" }),
  req("R-dismissed", "dismissed"),
];
const MAP: Record<string, InboxRequest["status"][]> = {
  unanswered: ["open", "proposed", "delivery_failed"],
  answered: ["confirmed", "resolved"],
  dismissed: ["dismissed"],
};

const api = vi.hoisted(() => ({
  fetchInboxRequests: vi.fn(),
  fetchInboxComments: vi.fn(),
  dismissInboxRequest: vi.fn(),
  archiveInboxRequest: vi.fn(),
  retryInboxNotice: vi.fn(),
  fetchInboxRequestHistory: vi.fn(),
}));
vi.mock("./api", async (orig) => ({ ...(await orig<object>()), ...api }));

import InboxActivity from "./InboxActivity";

beforeEach(() => {
  vi.clearAllMocks();
  api.fetchInboxComments.mockResolvedValue({ overall: [], worktrees: [] });
  api.fetchInboxRequests.mockImplementation(async (_id: string, filter?: string) => ({
    requests: !filter || filter === "all" ? ALL : ALL.filter((r) => MAP[filter].includes(r.status)),
  }));
  api.dismissInboxRequest.mockResolvedValue({});
  api.archiveInboxRequest.mockResolvedValue({});
});

function setup() {
  render(<InboxActivity draftId="01D" worktrees={[]} lastJob={null} onchanged={() => {}} />);
}

const shown = () =>
  screen.queryAllByRole("article").map((a) => a.getAttribute("aria-label"));

describe("InboxActivity filters (TS-36)", () => {
  it("offers exactly Unanswered, Answered, Dismissed and All", async () => {
    setup();
    const bar = await screen.findByRole("group", { name: "Filter requests" });
    const names = within(bar).getAllByRole("button").map((b) => b.textContent);
    expect(names).toEqual(["Unanswered", "Answered", "Dismissed", "All"]);
  });

  it("shows only the mapped statuses for each filter", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByRole("article", { name: "Request: Title R-open" });
    expect(shown()).toHaveLength(4);
    const bar = screen.getByRole("group", { name: "Filter requests" });

    await user.click(within(bar).getByRole("button", { name: "Unanswered" }));
    expect(api.fetchInboxRequests).toHaveBeenLastCalledWith("01D", "unanswered");
    await screen.findByRole("article", { name: "Request: Title R-failed" });
    expect(shown()).toEqual(["Request: Title R-open", "Request: Title R-failed"]);
    expect(within(bar).getByRole("button", { name: "Unanswered" })).toHaveAttribute("aria-pressed", "true");

    await user.click(within(bar).getByRole("button", { name: "Answered" }));
    await screen.findByRole("article", { name: "Request: Title R-resolved" });
    expect(shown()).toEqual(["Request: Title R-resolved"]);

    await user.click(within(bar).getByRole("button", { name: "Dismissed" }));
    await screen.findByRole("article", { name: "Request: Title R-dismissed" });
    expect(shown()).toEqual(["Request: Title R-dismissed"]);

    await user.click(within(bar).getByRole("button", { name: "All" }));
    await screen.findByRole("article", { name: "Request: Title R-open" });
    expect(shown()).toHaveLength(4);
  });

  it("dismisses through the operator route and reloads", async () => {
    const user = userEvent.setup();
    setup();
    const card = await screen.findByRole("article", { name: "Request: Title R-open" });
    await user.click(within(card).getByRole("button", { name: "Dismiss" }));
    expect(api.dismissInboxRequest).toHaveBeenCalledWith("01D", "R-open");
  });
});
