import { beforeAll, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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
  const base = { key: 1, projectPath: "/code/acme", branch: "x", prompt: "go" };

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
});
