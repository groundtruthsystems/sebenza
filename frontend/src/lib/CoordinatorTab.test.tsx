import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import CoordinatorTab from "./CoordinatorTab";
import { ApiRefusal } from "./solutionApi";
import { fakeApi } from "./solutionTestApi";

vi.mock("./api", () => ({
  addInboxNote: vi.fn(),
  deleteInboxNote: vi.fn(),
  reorderInboxNotes: vi.fn(),
  updateInboxNote: vi.fn(),
}));

afterEach(cleanup);

const notes = [
  { id: "n1", type: "general", title: "Brief", body: "Invoices", hash: "nh1" },
  { id: "n2", type: "constraints", title: "Constraints", body: "- EUR", hash: "nh2" },
];

function setup(api = fakeApi()) {
  const onStale = vi.fn();
  render(
    <CoordinatorTab
      draftId="01ID"
      notes={notes}
      listHash="lh"
      coordinatorNotes={{ n2: "nh2", n1: "stale-hash" }}
      onNotesChanged={vi.fn()}
      onStale={onStale}
      api={api}
      pollMs={60_000}
    />,
  );
  return { api, onStale };
}

describe("CoordinatorTab", () => {
  // TS-04 (UI half): the persisted transcript shows after a reload.
  it("shows the persisted transcript on open", async () => {
    const api = fakeApi({
      transcript: vi.fn().mockResolvedValue({
        entries: [
          { turnId: "T1", kind: "operator", text: "We need invoices." },
          { turnId: "T1", kind: "coordinator", agent: "codex", text: "Noted, **three** notes." },
        ],
        job: { running: false },
      }),
    });
    setup(api);
    expect(await screen.findByText("We need invoices.")).toBeInTheDocument();
    expect(screen.getByText("three").tagName).toBe("STRONG");
    expect(screen.getByText("Coordinator (codex)")).toBeInTheDocument();
  });

  // TS-05 (UI): the badge shows only while the coordinator's write stands.
  it("badges notes the coordinator wrote and has not been edited since", async () => {
    setup();
    const constraints = await screen.findByRole("region", { name: "Note: Constraints" });
    expect(within(constraints).getByText("coordinator")).toBeInTheDocument();
    const brief = screen.getByRole("region", { name: "Note: Brief" });
    expect(within(brief).queryByText("coordinator")).toBeNull();
  });

  it("applies a proposed note once", async () => {
    const api = fakeApi({
      transcript: vi
        .fn()
        .mockResolvedValueOnce({
          entries: [
            {
              turnId: "T1",
              kind: "coordinator",
              text: "One note.",
              noteProposals: [{ note_id: null, type: "acceptance_criteria", title: "AC", body: "1. x" }],
            },
          ],
          job: { running: false },
        })
        .mockResolvedValue({
          entries: [
            { turnId: "T1", kind: "coordinator", text: "One note.", noteProposals: [{ note_id: null, type: "acceptance_criteria", title: "AC", body: "1. x" }] },
            { turnId: "T1", kind: "applied", index: 0 },
          ],
          job: { running: false },
        }),
    });
    const { onStale } = setup(api);
    await userEvent.click(await screen.findByRole("button", { name: "Apply" }));
    expect(api.apply).toHaveBeenCalledWith("01ID", "T1", 0);
    expect(onStale).toHaveBeenCalled();
    expect(await screen.findByRole("button", { name: "Applied" })).toBeDisabled();
  });

  // TS-45 (UI): a secret-looking message warns, and sends only on confirm.
  it("warns before sending a secret-looking message", async () => {
    const say = vi
      .fn()
      .mockRejectedValueOnce(new ApiRefusal(422, "scan_warning", "the message looks like it contains AWS access key; confirm to send it anyway"))
      .mockResolvedValue({ turnId: "T9" });
    const api = fakeApi({ say });
    setup(api);
    await userEvent.type(screen.getByLabelText("Message the coordinator"), "key AKIAABCDEFGHIJKLMNOP");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("AWS access key");
    expect(say).toHaveBeenLastCalledWith("01ID", "key AKIAABCDEFGHIJKLMNOP", false);
    await userEvent.click(screen.getByRole("button", { name: "Send anyway" }));
    await waitFor(() => expect(say).toHaveBeenLastCalledWith("01ID", "key AKIAABCDEFGHIJKLMNOP", true));
  });

  // TS-49 (chat): agent text cannot run script.
  it("renders agent text without executing markup", async () => {
    const api = fakeApi({
      transcript: vi.fn().mockResolvedValue({
        entries: [{ turnId: "T1", kind: "coordinator", text: '<img src=x onerror="window.__pwned=1"><script>window.__pwned=2</script>ok' }],
        job: { running: false },
      }),
    });
    setup(api);
    const msg = await screen.findByRole("listitem", { name: "coordinator message" });
    expect(msg.querySelector("script")).toBeNull();
    expect(msg.querySelector("[onerror]")).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });
});
