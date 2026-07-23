import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import SettingsDialog from "./SettingsDialog";
import { useStore } from "../store";
import type { AgentDetails, AgentSummary, AppConfig } from "./types";

vi.mock("./api", () => ({
  api: {
    fetchConfig: vi.fn(),
    setAutoRemoveOnMerge: vi.fn(),
  },
  fetchAgents: vi.fn(),
  createAgent: vi.fn(),
  updateAgent: vi.fn(),
  deleteAgent: vi.fn(),
  validateAgent: vi.fn(),
}));

import { api, createAgent, deleteAgent, fetchAgents, validateAgent } from "./api";

const originalDialogShowModal = HTMLDialogElement.prototype.showModal;
const originalDialogClose = HTMLDialogElement.prototype.close;

function createAgentDetails(overrides: Partial<AgentDetails> = {}): AgentDetails {
  return {
    id: "gemini",
    label: "Gemini CLI",
    kind: "custom",
    capabilities: {
      terminal: true,
      inAppChat: false,
      conversationHistory: false,
      interrupt: false,
      resume: true,
    },
    startCommand: 'gemini --prompt "${PROMPT}"',
    resumeCommand: 'gemini resume --branch "${BRANCH}"',
    ...overrides,
  };
}

function createAgentSummary(overrides: Partial<AgentSummary> = {}): AgentSummary {
  return {
    id: "gemini",
    label: "Gemini CLI",
    kind: "custom",
    capabilities: {
      terminal: true,
      inAppChat: false,
      conversationHistory: false,
      interrupt: false,
      resume: true,
    },
    ...overrides,
  };
}

function createConfig(overrides: Partial<AppConfig> = {}): AppConfig {
  return {
    name: "repo",
    services: [],
    profiles: [{ name: "default" }],
    agents: [],
    launchers: [],
    defaultProfileName: "default",
    defaultAgentId: "claude",
    autoName: false,
    startupEnvs: {},
    linkedRepos: [],
    autoRemoveOnMerge: false,
    projectDir: "/repo",
    mainBranch: "main",
    ...overrides,
  };
}

function renderDialog() {
  return render(
    <SettingsDialog
      autoRemoveOnMerge={false}
      onautoremovechange={vi.fn()}
      onagentschange={vi.fn()}
      onclose={vi.fn()}
    />,
  );
}

describe("SettingsDialog agent management", () => {
  beforeEach(() => {
    useStore.setState({ theme: "github-dark", useWebChatUi: false, sshHost: "" });
    HTMLDialogElement.prototype.showModal = function showModal() {
      this.setAttribute("open", "");
    };
    HTMLDialogElement.prototype.close = function close() {
      this.removeAttribute("open");
    };
  });

  afterEach(() => {
    HTMLDialogElement.prototype.showModal = originalDialogShowModal;
    HTMLDialogElement.prototype.close = originalDialogClose;
    cleanup();
    vi.clearAllMocks();
    useStore.setState({ theme: "github-dark", useWebChatUi: false, sshHost: "" });
  });

  it("shows only custom agents in the list", async () => {
    vi.mocked(fetchAgents).mockResolvedValue([
      createAgentDetails({ id: "claude", label: "Claude", kind: "builtin", startCommand: null, resumeCommand: null, capabilities: {
        terminal: true,
        inAppChat: true,
        conversationHistory: true,
        interrupt: true,
        resume: true,
      } }),
      createAgentDetails(),
    ]);

    renderDialog();

    await screen.findByText("Gemini CLI");
    expect(screen.queryByText("Claude")).not.toBeInTheDocument();
    expect(screen.getByText('gemini --prompt "${PROMPT}"')).toBeInTheDocument();
  });

  it("shows an empty state when no custom agents are configured", async () => {
    vi.mocked(fetchAgents).mockResolvedValue([
      createAgentDetails({ id: "claude", label: "Claude", kind: "builtin", startCommand: null, resumeCommand: null, capabilities: {
        terminal: true,
        inAppChat: true,
        conversationHistory: true,
        interrupt: true,
        resume: true,
      } }),
    ]);

    renderDialog();

    expect(await screen.findByText("No custom agents setup")).toBeInTheDocument();
    expect(screen.queryByText("Claude")).not.toBeInTheDocument();
  });

  it("validates, creates, and deletes custom agents", async () => {
    const onagentschange = vi.fn();
    vi.mocked(fetchAgents)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([createAgentDetails()])
      .mockResolvedValueOnce([]);
    vi.mocked(createAgent).mockResolvedValue({ agent: createAgentDetails() });
    vi.mocked(validateAgent).mockResolvedValue({ normalizedId: "gemini-cli", warnings: [] });
    vi.mocked(deleteAgent).mockResolvedValue();
    vi.mocked(api.fetchConfig)
      .mockResolvedValueOnce(createConfig({ agents: [createAgentSummary()] }))
      .mockResolvedValueOnce(createConfig({ agents: [] }));

    render(
      <SettingsDialog
        autoRemoveOnMerge={false}
        onautoremovechange={vi.fn()}
        onagentschange={onagentschange}
        onclose={vi.fn()}
      />,
    );

    await screen.findByText("Add agent");
    fireEvent.click(screen.getByRole("button", { name: "Add agent" }));
    fireEvent.input(screen.getByLabelText("Agent name"), { target: { value: "Gemini CLI" } });
    fireEvent.input(screen.getByLabelText("Start command"), { target: { value: 'gemini --prompt "${PROMPT}"' } });
    fireEvent.click(screen.getByRole("button", { name: "Test" }));

    await waitFor(() => {
      expect(validateAgent).toHaveBeenCalledWith({
        label: "Gemini CLI",
        startCommand: 'gemini --prompt "${PROMPT}"',
      });
    });
    expect(await screen.findByText("Configuration looks good.")).toBeInTheDocument();

    fireEvent.click(screen.getAllByRole("button", { name: "Save" }).at(-1)!);

    await waitFor(() => {
      expect(createAgent).toHaveBeenCalledWith({
        label: "Gemini CLI",
        startCommand: 'gemini --prompt "${PROMPT}"',
      });
    });
    await waitFor(() => {
      expect(onagentschange).toHaveBeenCalledWith([createAgentSummary()]);
    });

    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));

    await waitFor(() => {
      expect(deleteAgent).toHaveBeenCalledWith("gemini");
    });
    await waitFor(() => {
      expect(onagentschange).toHaveBeenCalledWith([]);
    });
  });
});
