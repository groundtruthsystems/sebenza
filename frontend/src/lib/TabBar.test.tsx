import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import TabBar from "./TabBar";
import type { AgentSummary, WorktreeTab } from "./types";

afterEach(cleanup);

function tab(overrides: Partial<WorktreeTab> = {}): WorktreeTab {
  return {
    tabId: "root",
    kind: "root",
    label: "Root",
    seq: null,
    sessionId: null,
    createdAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function agent(id: string, label: string, kind: "builtin" | "custom"): AgentSummary {
  return {
    id,
    label,
    kind,
    capabilities: {
      terminal: true,
      inAppChat: kind === "builtin",
      conversationHistory: kind === "builtin",
      interrupt: kind === "builtin",
      resume: kind === "builtin",
      fork: kind === "builtin",
      pinnableSessionId: kind === "builtin",
      permissionInterception: false,
    },
  };
}

const ALL_AGENTS = [
  agent("claude", "Claude", "builtin"),
  agent("codex", "Codex", "builtin"),
  agent("goose", "Goose", "custom"),
  agent("opencode", "OpenCode", "custom"),
];

function baseProps() {
  return {
    tabs: [tab()],
    activeTabId: "root",
    agents: ALL_AGENTS,
    oncreate: vi.fn(),
    oncreateshell: vi.fn(),
    oncreateagent: vi.fn(),
    onselect: vi.fn(),
    ondelete: vi.fn(),
  };
}

async function openMenu(user: ReturnType<typeof userEvent.setup>): Promise<void> {
  await user.click(screen.getByLabelText("New tab"));
}

describe("TabBar", () => {
  it("offers Fork only when the agent can fork", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<TabBar {...baseProps()} canFork />);
    await openMenu(user);
    expect(screen.getByRole("button", { name: "Fork" })).toBeTruthy();
    unmount();

    render(<TabBar {...baseProps()} canFork={false} />);
    await openMenu(user);
    expect(screen.queryByRole("button", { name: "Fork" })).toBeNull();
  });

  it("opens the menu even when forking is unavailable", async () => {
    // Regression: the "+" used to skip the menu and create a terminal directly
    // for non-forkable agents, which hid the provider list entirely.
    const user = userEvent.setup();
    const props = baseProps();
    render(<TabBar {...props} canFork={false} />);
    await openMenu(user);
    expect(props.oncreateshell).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Terminal" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /New session/ })).toBeTruthy();
  });

  it("lists every configured agent under New session, custom ones included", async () => {
    const user = userEvent.setup();
    render(<TabBar {...baseProps()} canFork />);
    await openMenu(user);
    // Nested list is collapsed until asked for.
    expect(screen.queryByRole("button", { name: "Goose" })).toBeNull();
    await user.click(screen.getByRole("button", { name: /New session/ }));
    for (const label of ["Claude", "Codex", "Goose", "OpenCode"]) {
      expect(screen.getByRole("button", { name: label })).toBeTruthy();
    }
  });

  it("calls oncreateagent with the chosen agent id and closes the menu", async () => {
    const user = userEvent.setup();
    const props = baseProps();
    render(<TabBar {...props} canFork />);
    await openMenu(user);
    await user.click(screen.getByRole("button", { name: /New session/ }));
    await user.click(screen.getByRole("button", { name: "Goose" }));

    expect(props.oncreateagent).toHaveBeenCalledWith("goose");
    expect(screen.queryByRole("button", { name: "Terminal" })).toBeNull();
  });

  it("resets the nested list when the menu is reopened", async () => {
    const user = userEvent.setup();
    render(<TabBar {...baseProps()} canFork />);
    await openMenu(user);
    await user.click(screen.getByRole("button", { name: /New session/ }));
    expect(screen.getByRole("button", { name: "Goose" })).toBeTruthy();

    await openMenu(user); // collapse
    await openMenu(user); // reopen
    expect(screen.queryByRole("button", { name: "Goose" })).toBeNull();
  });

  it("routes Fork and Terminal to their own handlers", async () => {
    const user = userEvent.setup();
    const props = baseProps();
    render(<TabBar {...props} canFork />);
    await openMenu(user);
    await user.click(screen.getByRole("button", { name: "Fork" }));
    expect(props.oncreate).toHaveBeenCalledTimes(1);

    await openMenu(user);
    await user.click(screen.getByRole("button", { name: "Terminal" }));
    expect(props.oncreateshell).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape, backing out of the nested list first", async () => {
    const user = userEvent.setup();
    render(<TabBar {...baseProps()} canFork />);
    await openMenu(user);
    await user.click(screen.getByRole("button", { name: /New session/ }));

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("button", { name: "Goose" })).toBeNull();
    expect(screen.getByRole("button", { name: "Terminal" })).toBeTruthy();

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("button", { name: "Terminal" })).toBeNull();
  });

  it("closes when clicking outside the menu", async () => {
    const user = userEvent.setup();
    render(
      <div>
        <TabBar {...baseProps()} canFork />
        <button type="button">elsewhere</button>
      </div>,
    );
    await openMenu(user);
    await user.click(screen.getByRole("button", { name: "elsewhere" }));
    expect(screen.queryByRole("button", { name: "Terminal" })).toBeNull();
  });

  it("hides New session when no agents are configured", async () => {
    const user = userEvent.setup();
    render(<TabBar {...baseProps()} agents={[]} canFork />);
    await openMenu(user);
    expect(screen.queryByRole("button", { name: /New session/ })).toBeNull();
    expect(screen.getByRole("button", { name: "Terminal" })).toBeTruthy();
  });

  it("disables every menu item while busy", async () => {
    const user = userEvent.setup();
    render(<TabBar {...baseProps()} canFork busy />);
    // The "+" itself is disabled, so open it before flipping busy is impossible;
    // assert the disabled trigger instead.
    expect(screen.getByLabelText("New tab")).toHaveProperty("disabled", true);
    await user.click(screen.getByLabelText("New tab"));
    expect(screen.queryByRole("button", { name: "Terminal" })).toBeNull();
  });

  it("renders a close button for fork, shell and agent tabs but not root", () => {
    const props = baseProps();
    render(
      <TabBar
        {...props}
        tabs={[
          tab(),
          tab({ tabId: "fork-1", kind: "fork", label: "Fork 1", seq: 1 }),
          tab({ tabId: "shell-1", kind: "shell", label: "Shell" }),
          tab({ tabId: "agent-codex-1", kind: "agent", label: "Codex", agent: "codex" }),
        ]}
      />,
    );
    expect(screen.queryByLabelText("Close Root")).toBeNull();
    expect(screen.getByLabelText("Close Fork 1")).toBeTruthy();
    expect(screen.getByLabelText("Close Shell")).toBeTruthy();
    expect(screen.getByLabelText("Close Codex")).toBeTruthy();
  });

  it("selects and deletes tabs by id", async () => {
    const user = userEvent.setup();
    const props = baseProps();
    render(
      <TabBar
        {...props}
        tabs={[tab(), tab({ tabId: "agent-goose-1", kind: "agent", label: "Goose" })]}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Goose" }));
    expect(props.onselect).toHaveBeenCalledWith("agent-goose-1");

    await user.click(screen.getByLabelText("Close Goose"));
    expect(props.ondelete).toHaveBeenCalledWith("agent-goose-1");
  });
});
