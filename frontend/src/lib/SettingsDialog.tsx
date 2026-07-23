import { useCallback, useEffect, useState, type ChangeEvent, type FormEvent } from "react";
import { errorMessage } from "./utils";
import BaseDialog from "./BaseDialog";
import Btn from "./Btn";
import Toggle from "./Toggle";
import ConfirmDialog from "./ConfirmDialog";
import AgentEditorDialog from "./AgentEditorDialog";
import { api, createAgent, deleteAgent, fetchAgents, updateAgent, validateAgent } from "./api";
import type { AgentDetails, AgentSummary, UpsertCustomAgentRequest } from "./types";
import { useStore } from "../store";

interface AgentEditorState {
  mode: "create" | "edit";
  agentId?: string;
  title: string;
  initialValue: {
    label: string;
    startCommand: string;
    resumeCommand: string;
  };
}

export default function SettingsDialog({
  autoRemoveOnMerge,
  onautoremovechange,
  onagentschange,
  onclose,
}: {
  autoRemoveOnMerge: boolean;
  onautoremovechange: (enabled: boolean) => void;
  onagentschange: (agents: AgentSummary[]) => void;
  onclose: () => void;
}) {
  const storedSshHost = useStore((s) => s.sshHost);
  const setSshHost = useStore((s) => s.setSshHost);

  const [sshHost, setSshHostInput] = useState(storedSshHost);
  const [pendingAutoRemove, setPendingAutoRemove] = useState<boolean | null>(null);
  const autoRemove = pendingAutoRemove ?? autoRemoveOnMerge;
  const [autoRemoveSaving, setAutoRemoveSaving] = useState(false);

  const [agents, setAgents] = useState<AgentDetails[]>([]);
  const customAgents = agents.filter((agent) => agent.kind === "custom");
  const [agentsLoading, setAgentsLoading] = useState(true);
  const [agentsError, setAgentsError] = useState<string | null>(null);
  const [editor, setEditor] = useState<AgentEditorState | null>(null);
  const [deleteCandidate, setDeleteCandidate] = useState<AgentDetails | null>(null);
  const [deletingAgentId, setDeletingAgentId] = useState<string | null>(null);

  const loadAgentList = useCallback(async (): Promise<void> => {
    setAgentsLoading(true);
    setAgentsError(null);

    try {
      setAgents(await fetchAgents());
    } catch (err) {
      setAgentsError(errorMessage(err));
    } finally {
      setAgentsLoading(false);
    }
  }, []);

  function syncAgentSummaries(): void {
    api
      .fetchConfig()
      .then((config) => {
        onagentschange(config.agents);
      })
      .catch(() => {});
  }

  useEffect(() => {
    void loadAgentList();
  }, [loadAgentList]);

  function handleAutoRemoveToggle(enabled: boolean): void {
    setPendingAutoRemove(enabled);
    setAutoRemoveSaving(true);
    api
      .setAutoRemoveOnMerge({ body: { enabled } })
      .then((result) => {
        onautoremovechange(result.enabled);
      })
      .finally(() => {
        setPendingAutoRemove(null);
        setAutoRemoveSaving(false);
      });
  }

  function handleSave(): void {
    const trimmed = sshHost.trim();
    setSshHost(trimmed);
    onclose();
  }

  function openCreateAgentEditor(): void {
    setEditor({
      mode: "create",
      title: "Add custom agent",
      initialValue: {
        label: "",
        startCommand: "",
        resumeCommand: "",
      },
    });
  }

  function openEditAgentEditor(agent: AgentDetails): void {
    setEditor({
      mode: "edit",
      agentId: agent.id,
      title: `Edit ${agent.label}`,
      initialValue: {
        label: agent.label,
        startCommand: agent.startCommand ?? "",
        resumeCommand: agent.resumeCommand ?? "",
      },
    });
  }

  function openDuplicateAgentEditor(agent: AgentDetails): void {
    setEditor({
      mode: "create",
      title: `Duplicate ${agent.label}`,
      initialValue: {
        label: `${agent.label} Copy`,
        startCommand: agent.startCommand ?? "",
        resumeCommand: agent.resumeCommand ?? "",
      },
    });
  }

  async function handleSaveAgent(input: UpsertCustomAgentRequest): Promise<void> {
    if (!editor) return;

    if (editor.mode === "edit" && editor.agentId) {
      await updateAgent(editor.agentId, input);
    } else {
      await createAgent(input);
    }

    await loadAgentList();
    syncAgentSummaries();
    setEditor(null);
  }

  function handleValidateAgent(input: UpsertCustomAgentRequest) {
    return validateAgent(input);
  }

  async function handleDeleteAgent(): Promise<void> {
    if (!deleteCandidate) return;
    setDeletingAgentId(deleteCandidate.id);

    try {
      await deleteAgent(deleteCandidate.id);
      await loadAgentList();
      syncAgentSummaries();
      setDeleteCandidate(null);
    } finally {
      setDeletingAgentId(null);
    }
  }

  return (
    <>
      <BaseDialog onclose={onclose} wide>
        <form
          onSubmit={(event: FormEvent<HTMLFormElement>) => {
            event.preventDefault();
            handleSave();
          }}
        >
          <h2 className="text-base mb-4">Settings</h2>

          <div className="mb-5">
            <span className="block text-xs text-muted mb-2">Agents</span>
            <div className="rounded-lg border border-edge bg-surface/40 p-3">
              <div className="mb-3 flex items-center justify-between gap-2">
                <div>
                  <p className="text-[13px] text-primary">Custom agents</p>
                  <p className="mt-0.5 text-[11px] text-muted">
                    Add terminal agents that Sebenza can launch from the dashboard.
                  </p>
                </div>
                <Btn type="button" variant="cta" onClick={openCreateAgentEditor}>
                  Add agent
                </Btn>
              </div>

              {agentsLoading ? (
                <p className="text-[12px] text-muted">Loading agents...</p>
              ) : agentsError ? (
                <p className="text-[12px] text-danger">{agentsError}</p>
              ) : customAgents.length === 0 ? (
                <p className="text-[12px] text-muted">No custom agents setup</p>
              ) : (
                <div className="space-y-2">
                  {customAgents.map((agent) => (
                    <div
                      key={agent.id}
                      className="rounded-lg border border-edge bg-surface px-3 py-2.5"
                    >
                      <div className="flex items-start justify-between gap-3">
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-1.5">
                            <span className="text-[13px] text-primary">{agent.label}</span>
                          </div>
                          <p className="mt-1 text-[11px] text-muted font-mono break-all">
                            {agent.startCommand}
                          </p>
                          {agent.resumeCommand && (
                            <p className="mt-1 text-[11px] text-muted font-mono break-all">
                              Resume: {agent.resumeCommand}
                            </p>
                          )}
                        </div>

                        <div className="flex shrink-0 gap-2 text-[11px]">
                          <button
                            type="button"
                            className="text-accent hover:underline"
                            onClick={() => openEditAgentEditor(agent)}
                          >
                            Edit
                          </button>
                          <button
                            type="button"
                            className="text-accent hover:underline"
                            onClick={() => openDuplicateAgentEditor(agent)}
                          >
                            Duplicate
                          </button>
                          <button
                            type="button"
                            className="text-danger hover:underline disabled:opacity-60"
                            disabled={deletingAgentId === agent.id}
                            onClick={() => setDeleteCandidate(agent)}
                          >
                            Delete
                          </button>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>

          <div className="mb-5">
            <span className="block text-xs text-muted mb-2">GitHub</span>
            <div className="flex items-center justify-between gap-3 px-3 py-2 rounded-md border border-edge bg-surface">
              <div>
                <span className="text-[13px] text-primary">Auto-remove on merge</span>
                <p className="text-[11px] text-muted mt-0.5">
                  Automatically remove worktrees when their PR is merged on GitHub.
                </p>
              </div>

              <Toggle
                checked={autoRemove}
                disabled={autoRemoveSaving}
                onToggle={handleAutoRemoveToggle}
                aria-label="Auto-remove worktrees on PR merge"
              />
            </div>
          </div>

          <div className="mb-4">
            <label className="block text-xs text-muted mb-1.5" htmlFor="ssh-host">
              SSH Host <span className="opacity-60">(for "Open in Zed")</span>
            </label>
            <input
              id="ssh-host"
              type="text"
              className="w-full px-2.5 py-1.5 rounded-md border border-edge bg-surface text-primary text-[13px] placeholder:text-muted/50 outline-none focus:border-accent"
              placeholder="e.g. devbox or 10.0.0.5"
              value={sshHost}
              onChange={(e: ChangeEvent<HTMLInputElement>) => setSshHostInput(e.currentTarget.value)}
            />
            <p className="text-[11px] text-muted mt-1.5">
              Must match an entry in your local{" "}
              <code className="text-accent/80">~/.ssh/config</code>. Leave empty for local mode.
            </p>
          </div>
          <div className="flex justify-end gap-2">
            <Btn type="button" onClick={onclose}>
              Cancel
            </Btn>
            <Btn type="submit" variant="cta">
              Save
            </Btn>
          </div>
        </form>
      </BaseDialog>

      {editor && (
        <AgentEditorDialog
          title={editor.title}
          initialValue={editor.initialValue}
          onsave={handleSaveAgent}
          onvalidate={handleValidateAgent}
          onclose={() => setEditor(null)}
        />
      )}

      {deleteCandidate && (
        <ConfirmDialog
          message={`Delete agent "${deleteCandidate.label}"?`}
          onconfirm={() => {
            void handleDeleteAgent();
          }}
          oncancel={() => {
            setDeleteCandidate(null);
          }}
        />
      )}
    </>
  );
}
