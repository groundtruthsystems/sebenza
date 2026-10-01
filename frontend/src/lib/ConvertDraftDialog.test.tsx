import { beforeAll, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
const instructions = vi.fn();
vi.mock("./api", () => ({
  fetchBaseBranchesFor: () => Promise.resolve(["main", "develop"]),
  requestConvertInstructions: (...a: unknown[]) => instructions(...a),
}));

import ConvertDraftDialog, { MAX_TARGETS, rowError } from "./ConvertDraftDialog";
import type { ProjectSummary } from "./types";

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

const projects: ProjectSummary[] = [
  { prefix: "acme", name: "acme", path: "/code/acme", active: false },
  { prefix: "beta", name: "beta", path: "/code/beta", active: false },
];

function setup(over: Partial<Parameters<typeof ConvertDraftDialog>[0]> = {}) {
  const onconvert = vi.fn();
  render(
    <ConvertDraftDialog
      draftTitle="Claims rework"
      projects={projects}
      onconvert={onconvert}
      oncancel={vi.fn()}
      {...over}
    />,
  );
  return { onconvert };
}

describe("rowError", () => {
  const base = {
    key: 1,
    projectPath: "/code/acme",
    branch: "x",
    baseBranch: "",
    prompt: "go",
  };

  it("accepts a complete row", () => {
    expect(rowError(base, [base])).toBeNull();
  });

  it("requires a branch and a prompt", () => {
    expect(rowError({ ...base, branch: "  " }, [])).toMatch(/branch/i);
    expect(rowError({ ...base, prompt: "  " }, [])).toMatch(/prompt/i);
  });

  it("rejects a branch with spaces, which git would refuse", () => {
    expect(rowError({ ...base, branch: "two words" }, [])).toMatch(/spaces/i);
  });

  it("catches the same branch twice in one project", () => {
    const a = { ...base, key: 1 };
    const b = { ...base, key: 2 };
    expect(rowError(b, [a, b])).toMatch(/twice/i);
  });

  it("allows the same branch in different projects", () => {
    const a = { ...base, key: 1, projectPath: "/code/acme" };
    const b = { ...base, key: 2, projectPath: "/code/beta" };
    expect(rowError(b, [a, b])).toBeNull();
  });
});

describe("ConvertDraftDialog", () => {
  it("cannot convert an incomplete row", () => {
    setup();
    expect(screen.getByRole("button", { name: /Create 1 worktree/ })).toBeDisabled();
  });

  it("converts with the trimmed target", async () => {
    const user = userEvent.setup();
    const { onconvert } = setup();
    await user.type(screen.getByLabelText("Branch for target 1"), "  fix-scorer  ");
    await user.type(screen.getByLabelText("Prompt for target 1"), "  rewrite it  ");
    await user.click(screen.getByRole("button", { name: /Create 1 worktree/ }));
    expect(onconvert).toHaveBeenCalledWith([
      { projectPath: "/code/acme", branch: "fix-scorer", prompt: "rewrite it" },
    ]);
    // No baseBranch key at all: absent means the project's default, and an
    // empty string would be a different thing to validate server-side.
  });

  it("adds and removes targets, and never removes the last one", async () => {
    const user = userEvent.setup();
    setup();
    expect(screen.getByLabelText("Remove target 1")).toBeDisabled();

    await user.click(screen.getByRole("button", { name: /Add worktree/ }));
    expect(screen.getByLabelText("Branch for target 2")).toBeInTheDocument();
    expect(screen.getByLabelText("Remove target 1")).toBeEnabled();

    await user.click(screen.getByLabelText("Remove target 2"));
    expect(screen.queryByLabelText("Branch for target 2")).not.toBeInTheDocument();
  });

  it("stops at the server's cap", async () => {
    const user = userEvent.setup();
    setup();
    const add = screen.getByRole("button", { name: /Add worktree/ });
    for (let i = 1; i < MAX_TARGETS; i++) await user.click(add);
    expect(screen.getByLabelText(`Branch for target ${MAX_TARGETS}`)).toBeInTheDocument();
    expect(add).toBeDisabled();
  });

  it("pre-fills a second wave from the first", () => {
    setup({
      previousTargets: [
        { projectPath: "/code/beta", branch: "wave-one", prompt: "the first ask" },
      ],
    });
    expect(screen.getByLabelText("Branch for target 1")).toHaveValue("wave-one");
    expect(screen.getByLabelText("Prompt for target 1")).toHaveValue("the first ask");
  });

  it("says so when there is nowhere to create a worktree", () => {
    setup({ projects: [] });
    expect(screen.getByText(/No projects are registered/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Add worktree/ })).toBeDisabled();
  });

  it("keeps the rows when the server refuses, so they can be corrected", async () => {
    const user = userEvent.setup();
    setup({ error: "target 0: no registered project" });
    await user.type(screen.getByLabelText("Branch for target 1"), "keep-me");
    expect(screen.getByText(/no registered project/)).toBeInTheDocument();
    expect(screen.getByLabelText("Branch for target 1")).toHaveValue("keep-me");
  });

  it("sends a stated source branch and omits a blank one", async () => {
    const user = userEvent.setup();
    const { onconvert } = setup();
    await user.type(screen.getByLabelText("Branch for target 1"), "hotfix");
    await user.type(screen.getByLabelText("Prompt for target 1"), "patch it");
    await user.type(screen.getByLabelText("Source branch for target 1"), "develop");
    await user.click(screen.getByRole("button", { name: /Create 1 worktree/ }));
    expect(onconvert).toHaveBeenCalledWith([
      {
        projectPath: "/code/acme",
        branch: "hotfix",
        baseBranch: "develop",
        prompt: "patch it",
      },
    ]);
  });

  it("offers the project's branches as suggestions", async () => {
    setup();
    const field = screen.getByLabelText("Source branch for target 1");
    expect(field).toHaveAttribute("list", "base-branches-/code/acme");
    await screen.findByText("", { selector: 'option[value="develop"]' });
  });

  it("rejects a source branch with spaces", () => {
    const base = {
      key: 1,
      projectPath: "/code/acme",
      branch: "x",
      baseBranch: "two words",
      prompt: "go",
    };
    expect(rowError(base, [base])).toMatch(/source branch/i);
  });
});

describe("ConvertDraftDialog system instructions", () => {
  const response = (over: Record<string, unknown> = {}, target: Record<string, unknown> = {}) => ({
    jobId: "J1",
    status: "succeeded",
    fallback: false,
    error: null,
    targets: [
      {
        project: "/code/acme",
        branch: "fix-scorer",
        key: "acme/fix-scorer",
        systemInstruction: "Design the scorer change first.",
        sebenzaWorkspace: true,
        ...target,
      },
    ],
    advisories: [],
    ...over,
  });

  async function fill(user: ReturnType<typeof userEvent.setup>) {
    await user.type(screen.getByLabelText("Branch for target 1"), "fix-scorer");
    await user.type(screen.getByLabelText("Prompt for target 1"), "rewrite it");
  }

  it("asks the system agent for each target's instruction", async () => {
    const user = userEvent.setup();
    instructions.mockResolvedValue(response());
    setup({ draftId: "D1" });
    await fill(user);
    await user.click(screen.getByRole("button", { name: "Draft instructions" }));
    expect(instructions).toHaveBeenCalledWith("D1", [
      { project: "/code/acme", branch: "fix-scorer", prompt: "rewrite it" },
    ]);
    expect(await screen.findByLabelText("System instruction for target 1")).toHaveValue(
      "Design the scorer change first.",
    );
  });

  // TS-46: the operator's edit is what is submitted.
  it("submits the edited instruction with architect-first", async () => {
    const user = userEvent.setup();
    instructions.mockResolvedValue(response());
    const { onconvert } = setup({ draftId: "D1" });
    await fill(user);
    await user.click(screen.getByRole("button", { name: "Draft instructions" }));
    const field = await screen.findByLabelText("System instruction for target 1");
    await user.clear(field);
    await user.type(field, "Edited by the operator.");
    expect(screen.getByLabelText("Architect-first for target 1")).toBeEnabled();
    expect(screen.getByLabelText("Architect-first for target 1")).toHaveAttribute(
      "aria-checked",
      "true",
    );
    await user.click(screen.getByRole("button", { name: /Create 1 worktree/ }));
    expect(onconvert).toHaveBeenCalledWith([
      {
        projectPath: "/code/acme",
        branch: "fix-scorer",
        prompt: "rewrite it",
        systemInstruction: "Edited by the operator.",
        architectFirst: true,
      },
    ]);
  });

  it("can turn architect-first off for one target", async () => {
    const user = userEvent.setup();
    instructions.mockResolvedValue(response());
    const { onconvert } = setup({ draftId: "D1" });
    await fill(user);
    await user.click(screen.getByRole("button", { name: "Draft instructions" }));
    await user.click(await screen.findByLabelText("Architect-first for target 1"));
    await user.click(screen.getByRole("button", { name: /Create 1 worktree/ }));
    expect(onconvert.mock.calls[0][0][0].architectFirst).toBe(false);
  });

  it("disables architect-first, and says why, without a Sebenza workspace", async () => {
    const user = userEvent.setup();
    instructions.mockResolvedValue(response({}, { sebenzaWorkspace: false }));
    const { onconvert } = setup({ draftId: "D1" });
    await fill(user);
    await user.click(screen.getByRole("button", { name: "Draft instructions" }));
    expect(await screen.findByLabelText("Architect-first for target 1")).toBeDisabled();
    expect(screen.getByText(/no Sebenza workspace/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Create 1 worktree/ }));
    expect(onconvert.mock.calls[0][0][0].architectFirst).toBe(false);
  });

  it("falls back to the item note and operator prompt when the agent is unavailable", async () => {
    const user = userEvent.setup();
    instructions.mockResolvedValue(
      response(
        { jobId: null, status: "unavailable", fallback: true, error: "system agent is disabled" },
        { systemInstruction: null },
      ),
    );
    const { onconvert } = setup({ draftId: "D1" });
    await fill(user);
    await user.click(screen.getByRole("button", { name: "Draft instructions" }));
    expect(await screen.findByText(/system agent is disabled/)).toBeInTheDocument();
    expect(screen.getByText(/inbox note and your prompt/i)).toBeInTheDocument();
    expect(screen.queryByLabelText("System instruction for target 1")).toBeNull();
    await user.click(screen.getByRole("button", { name: /Create 1 worktree/ }));
    expect(onconvert.mock.calls[0][0][0].systemInstruction).toBeUndefined();
  });

  it("shows a failed job's fallback too", async () => {
    const user = userEvent.setup();
    instructions.mockResolvedValue(
      response(
        { status: "failed", fallback: true, error: "output did not match the schema" },
        { systemInstruction: null },
      ),
    );
    setup({ draftId: "D1" });
    await fill(user);
    await user.click(screen.getByRole("button", { name: "Draft instructions" }));
    expect(await screen.findByText(/did not match the schema/)).toBeInTheDocument();
  });

  it("drops an instruction whose row changed after drafting", async () => {
    const user = userEvent.setup();
    instructions.mockResolvedValue(response());
    const { onconvert } = setup({ draftId: "D1" });
    await fill(user);
    await user.click(screen.getByRole("button", { name: "Draft instructions" }));
    await screen.findByLabelText("System instruction for target 1");
    await user.type(screen.getByLabelText("Branch for target 1"), "-v2");
    expect(screen.queryByLabelText("System instruction for target 1")).toBeNull();
    expect(screen.getByText(/changed since/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Create 1 worktree/ }));
    await waitFor(() => expect(onconvert).toHaveBeenCalled());
    expect(onconvert.mock.calls[0][0][0].systemInstruction).toBeUndefined();
  });

  it("reports a refused instructions request", async () => {
    const user = userEvent.setup();
    instructions.mockRejectedValue(new Error("HTTP 401"));
    setup({ draftId: "D1" });
    await fill(user);
    await user.click(screen.getByRole("button", { name: "Draft instructions" }));
    expect(await screen.findByText(/HTTP 401/)).toBeInTheDocument();
  });
});
