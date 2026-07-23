import {
  useEffect,
  useState,
  type ChangeEvent,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import type {
  AgentId,
  AgentSummary,
  AvailableBranch,
  BuiltInAgentId,
  CreateWorktreeRequest,
  ProfileConfig,
  WorktreeCreateMode,
} from "./types";
import BaseDialog from "./BaseDialog";
import BranchSelector from "./BranchSelector";
import Btn from "./Btn";
import StartupEnvFields from "./StartupEnvFields";
import Toggle from "./Toggle";

const STORAGE_KEY = "wt-default-profile";
const AGENT_STORAGE_KEY = "wt-default-agents";
const MULTI_AGENT_STORAGE_KEY = "wt-default-multi-agents";
const ENV_STORAGE_KEY = "wt-default-envs";

function sameAgentIds(left: AgentId[], right: AgentId[]): boolean {
  return left.length === right.length && left.every((id, index) => id === right[index]);
}

function loadSavedMultiAgentMode(): boolean {
  return localStorage.getItem(MULTI_AGENT_STORAGE_KEY) === "true";
}

function loadSavedAgentIds(): AgentId[] {
  const saved = localStorage.getItem(AGENT_STORAGE_KEY);
  if (!saved) return [];

  try {
    const parsed = JSON.parse(saved) as unknown;
    if (Array.isArray(parsed)) {
      return parsed.filter(
        (entry): entry is AgentId => typeof entry === "string" && entry.trim().length > 0,
      );
    }
  } catch {
    if (saved.trim().length > 0) return [saved.trim()];
  }

  return saved.trim().length > 0 ? [saved.trim()] : [];
}

function loadSavedEnvs(
  startupEnvs: Record<string, string | boolean>,
): Record<string, string | boolean> {
  const savedEnvs = localStorage.getItem(ENV_STORAGE_KEY);
  if (!savedEnvs) return { ...startupEnvs };
  try {
    const parsed = JSON.parse(savedEnvs) as Record<string, string | boolean>;
    const filtered = Object.fromEntries(
      Object.entries(parsed).filter(([key]) => key in startupEnvs),
    );
    return { ...startupEnvs, ...filtered };
  } catch {
    return { ...startupEnvs };
  }
}

export default function CreateWorktreeDialog({
  profiles,
  agents = [],
  defaultProfileName = "",
  defaultAgentId = "claude",
  autoNameEnabled = false,
  initialBranch = "",
  initialPrompt = "",
  availableBranches = [],
  availableBranchesLoading = false,
  availableBranchesError = null,
  baseBranches = [],
  baseBranchesLoading = false,
  baseBranchesError = null,
  lockedBaseBranch = null,
  includeRemoteBranches = false,
  onIncludeRemoteBranches,
  startupEnvs = {},
  oncreate,
  oncancel,
}: {
  profiles: ProfileConfig[];
  agents?: AgentSummary[];
  defaultProfileName?: string;
  defaultAgentId?: BuiltInAgentId;
  autoNameEnabled?: boolean;
  initialBranch?: string;
  initialPrompt?: string;
  availableBranches?: AvailableBranch[];
  availableBranchesLoading?: boolean;
  availableBranchesError?: string | null;
  baseBranches?: AvailableBranch[];
  baseBranchesLoading?: boolean;
  baseBranchesError?: string | null;
  lockedBaseBranch?: string | null;
  includeRemoteBranches: boolean;
  onIncludeRemoteBranches?: (next: boolean) => void;
  startupEnvs?: Record<string, string | boolean>;
  oncreate: (request: CreateWorktreeRequest) => void;
  oncancel: () => void;
}) {
  const savedProfile = localStorage.getItem(STORAGE_KEY);
  const savedEnvs = localStorage.getItem(ENV_STORAGE_KEY);

  const [mode, setMode] = useState<WorktreeCreateMode>("new");
  const [newBranchName, setNewBranchName] = useState(initialBranch);
  const [prompt, setPrompt] = useState(initialPrompt);
  const [selectedExistingBranch, setSelectedExistingBranch] = useState("");
  const [selectedBaseBranch, setSelectedBaseBranch] = useState(lockedBaseBranch ?? "");
  const [multiAgentMode, setMultiAgentModeState] = useState(loadSavedMultiAgentMode);
  const [selectedAgentIds, setSelectedAgentIds] = useState<AgentId[]>(loadSavedAgentIds);
  const [profile, setProfile] = useState(savedProfile ?? "");
  const hasSavedDefaults =
    savedProfile != null ||
    localStorage.getItem(AGENT_STORAGE_KEY) != null ||
    localStorage.getItem(MULTI_AGENT_STORAGE_KEY) != null ||
    savedEnvs != null;
  const [saveDefault, setSaveDefault] = useState(hasSavedDefaults);
  const [envValues, setEnvValues] = useState<Record<string, string | boolean>>(() =>
    loadSavedEnvs(startupEnvs),
  );

  const availableAgentOptions = agents;
  const fallbackProfile = defaultProfileName || profiles[0]?.name || "default";
  const fallbackAgentId = availableAgentOptions.some((agent) => agent.id === defaultAgentId)
    ? defaultAgentId
    : (availableAgentOptions[0]?.id ?? "");

  const creatingMultipleAgents = multiAgentMode && selectedAgentIds.length > 1;
  const branchPreview =
    mode === "new" && creatingMultipleAgents && newBranchName.trim().length > 0
      ? selectedAgentIds.map((agentId) => `${agentId}-${newBranchName.trim()}`)
      : [];
  const canSubmit =
    selectedAgentIds.length > 0 && (mode === "new" || selectedExistingBranch.length > 0);

  useEffect(() => {
    if (!profiles.some((p) => p.name === profile)) {
      setProfile(fallbackProfile);
    }
  }, [profiles, profile, fallbackProfile]);

  useEffect(() => {
    const validAgentIds = new Set(availableAgentOptions.map((agent) => agent.id));
    const filteredIds = selectedAgentIds.filter((agentId) => validAgentIds.has(agentId));
    const nextSelectedAgentIds =
      filteredIds.length > 0
        ? filteredIds
        : validAgentIds.has(fallbackAgentId)
          ? [fallbackAgentId]
          : availableAgentOptions[0]
            ? [availableAgentOptions[0].id]
            : [];
    const normalizedAgentIds = multiAgentMode
      ? nextSelectedAgentIds
      : nextSelectedAgentIds.slice(0, 1);

    if (!sameAgentIds(selectedAgentIds, normalizedAgentIds)) {
      setSelectedAgentIds(normalizedAgentIds);
    }
  }, [availableAgentOptions, selectedAgentIds, multiAgentMode, fallbackAgentId]);

  useEffect(() => {
    if (creatingMultipleAgents && mode === "existing") {
      setMode("new");
      setSelectedExistingBranch("");
    }
  }, [creatingMultipleAgents, mode]);

  function setMultiAgentMode(enabled: boolean): void {
    setMultiAgentModeState(enabled);
    if (!enabled) {
      setSelectedAgentIds((ids) => ids.slice(0, 1));
    }
  }

  function toggleAgent(agentId: AgentId): void {
    if (!multiAgentMode) {
      setSelectedAgentIds([agentId]);
      return;
    }

    if (selectedAgentIds.includes(agentId)) {
      if (selectedAgentIds.length === 1) return;
      setSelectedAgentIds(selectedAgentIds.filter((id) => id !== agentId));
      return;
    }

    setSelectedAgentIds([...selectedAgentIds, agentId]);
  }

  function selectExistingBranch(name: string): void {
    setSelectedExistingBranch(name);
  }

  function openExistingBranchSelector(): void {
    setMode("existing");
    if (!selectedExistingBranch && initialBranch.trim().length > 0) {
      setSelectedExistingBranch(initialBranch.trim());
    }
  }

  function switchToNewBranchMode(): void {
    setMode("new");
  }

  function handleSubmit(e: FormEvent<HTMLFormElement>): void {
    e.preventDefault();
    if (!canSubmit) return;
    if (saveDefault) {
      localStorage.setItem(STORAGE_KEY, profile);
      localStorage.setItem(AGENT_STORAGE_KEY, JSON.stringify(selectedAgentIds));
      localStorage.setItem(MULTI_AGENT_STORAGE_KEY, String(multiAgentMode));
      localStorage.setItem(ENV_STORAGE_KEY, JSON.stringify(envValues));
    } else {
      localStorage.removeItem(STORAGE_KEY);
      localStorage.removeItem(AGENT_STORAGE_KEY);
      localStorage.removeItem(MULTI_AGENT_STORAGE_KEY);
      localStorage.removeItem(ENV_STORAGE_KEY);
    }
    const filteredEnvs: Record<string, string> = {};
    for (const [k, v] of Object.entries(envValues)) {
      if (typeof v === "boolean") {
        if (v) filteredEnvs[k] = "true";
      } else if (v) {
        filteredEnvs[k] = v;
      }
    }
    const trimmedPrompt = prompt.trim();
    const branchName = mode === "existing" ? selectedExistingBranch : newBranchName.trim();
    oncreate({
      mode,
      ...(branchName ? { branch: branchName } : {}),
      ...(mode === "new" && selectedBaseBranch ? { baseBranch: selectedBaseBranch } : {}),
      profile,
      agents: [...selectedAgentIds],
      ...(trimmedPrompt ? { prompt: trimmedPrompt } : {}),
      ...(Object.keys(filteredEnvs).length > 0 ? { envOverrides: filteredEnvs } : {}),
    });
  }

  return (
    <BaseDialog onclose={oncancel} className="md:max-w-[440px]">
      <form onSubmit={handleSubmit}>
        <h2 className="text-base mb-4">
          {lockedBaseBranch !== null ? "New Sub-Worktree" : "New Worktree"}
        </h2>
        <div className="mb-4">
          <label className="block text-xs text-muted mb-1.5" htmlFor="wt-prompt">
            Prompt <span className="opacity-60">(optional)</span>
          </label>
          <textarea
            id="wt-prompt"
            rows={4}
            autoFocus
            className="w-full px-2.5 py-1.5 rounded-md border border-edge bg-surface text-primary text-[13px] placeholder:text-muted/50 outline-none focus:border-accent resize-y"
            placeholder="Describe the task for the agent..."
            value={prompt}
            onChange={(e: ChangeEvent<HTMLTextAreaElement>) => setPrompt(e.currentTarget.value)}
            onKeyDown={(e: KeyboardEvent<HTMLTextAreaElement>) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                e.currentTarget.form?.requestSubmit();
              }
            }}
          />
        </div>
        <div className="mb-4">
          {mode === "new" ? (
            <>
              <label className="block text-xs text-muted mb-1.5" htmlFor="wt-name">
                Branch name <span className="opacity-60">(optional)</span>
              </label>
              <input
                id="wt-name"
                type="text"
                className="w-full px-2.5 py-1.5 rounded-md border border-edge bg-surface text-primary text-[13px] placeholder:text-muted/50 outline-none focus:border-accent"
                placeholder={
                  autoNameEnabled ? "generated from prompt if empty" : "auto-generated if empty"
                }
                value={newBranchName}
                onChange={(e: ChangeEvent<HTMLInputElement>) =>
                  setNewBranchName(e.currentTarget.value)
                }
              />
              {creatingMultipleAgents ? (
                <div className="mt-2 text-[11px] text-muted">
                  <p>A separate prefixed branch will be created for each selected agent.</p>
                  {branchPreview.length > 0 && (
                    <ul className="mt-1 space-y-0.5 font-mono text-[11px] text-primary/80">
                      {branchPreview.map((branch) => (
                        <li key={branch}>{branch}</li>
                      ))}
                    </ul>
                  )}
                </div>
              ) : (
                <button
                  type="button"
                  className="mt-2 text-[11px] text-accent hover:underline"
                  onClick={openExistingBranchSelector}
                >
                  Use existing branch
                </button>
              )}
            </>
          ) : (
            <>
              <BranchSelector
                label="Existing branch"
                selected={selectedExistingBranch}
                branches={availableBranches}
                loading={availableBranchesLoading}
                error={availableBranchesError}
                placeholder="Select a branch"
                initialOpen={true}
                inlineToggleLabel="include remote"
                inlineToggleAriaLabel="Include remote branches"
                inlineToggleChecked={includeRemoteBranches}
                oninlinetoggle={() => onIncludeRemoteBranches?.(!includeRemoteBranches)}
                onselect={selectExistingBranch}
              />
              <button
                type="button"
                className="mt-2 text-[11px] text-accent hover:underline"
                onClick={switchToNewBranchMode}
              >
                Create new branch instead
              </button>
              <p className="mt-2 text-[11px] text-muted">
                Removing this worktree will also delete the branch.
              </p>
            </>
          )}
        </div>
        {mode === "new" && (
          <div className="mb-4">
            <BranchSelector
              label="Base branch"
              selected={selectedBaseBranch}
              branches={baseBranches}
              loading={baseBranchesLoading}
              error={baseBranchesError}
              placeholder="Project main branch (default)"
              disabled={lockedBaseBranch !== null}
              onselect={(branch) => setSelectedBaseBranch(branch)}
            />
            {lockedBaseBranch !== null ? (
              <p className="mt-2 text-[11px] text-muted">
                Creating a sub-worktree based on{" "}
                <span className="font-mono">{lockedBaseBranch}</span>.
              </p>
            ) : (
              selectedBaseBranch && (
                <button
                  type="button"
                  className="mt-2 text-[11px] text-accent hover:underline"
                  onClick={() => setSelectedBaseBranch("")}
                >
                  Use project default branch instead
                </button>
              )
            )}
          </div>
        )}
        <StartupEnvFields
          startupEnvs={startupEnvs}
          envValues={envValues}
          onEnvValuesChange={setEnvValues}
        />
        <div className="mb-4">
          <div className="mb-2 flex items-center justify-between gap-2">
            <span className="text-xs text-muted">
              {multiAgentMode ? `Agents (${selectedAgentIds.length} selected)` : "Agent"}
            </span>
            <label className="flex items-center gap-2 text-[11px] text-muted cursor-pointer">
              <span>Multiple selection</span>
              <Toggle
                size="sm"
                checked={multiAgentMode}
                onToggle={setMultiAgentMode}
                aria-label="Enable multiple agent selection"
              />
            </label>
          </div>
          {creatingMultipleAgents && (
            <p className="mb-2 text-[11px] text-muted">Creates one worktree per agent.</p>
          )}
          {availableAgentOptions.length === 0 ? (
            <p className="rounded-lg border border-edge bg-surface px-3 py-2 text-[12px] text-muted">
              No agents available.
            </p>
          ) : (
            <div className="grid gap-2 sm:grid-cols-2">
              {availableAgentOptions.map((agentOption) => (
                <label
                  key={agentOption.id}
                  className={`flex items-start gap-2.5 p-2.5 rounded-lg border cursor-pointer text-[13px] transition-colors
                ${
                  selectedAgentIds.includes(agentOption.id)
                    ? "border-accent bg-accent/10"
                    : "border-edge hover:bg-hover"
                }`}
                >
                  <input
                    type={multiAgentMode ? "checkbox" : "radio"}
                    name={multiAgentMode ? undefined : "agent"}
                    checked={selectedAgentIds.includes(agentOption.id)}
                    onChange={() => toggleAgent(agentOption.id)}
                    className="mt-0.5 accent-[var(--accent)]"
                  />
                  <span className="min-w-0 flex-1 truncate text-primary">
                    {agentOption.label}
                  </span>
                </label>
              ))}
            </div>
          )}
        </div>
        {profiles.length > 1 && (
          <div className="flex flex-col gap-2 mb-6">
            {profiles.map((p) => (
              <label
                key={p.name}
                className={`flex items-center gap-2.5 p-2.5 rounded-lg border cursor-pointer text-[13px] transition-colors
              ${profile === p.name ? "border-accent bg-accent/10" : "border-edge hover:bg-hover"}`}
              >
                <input
                  type="radio"
                  name="profile"
                  value={p.name}
                  checked={profile === p.name}
                  onChange={() => setProfile(p.name)}
                  className="accent-[var(--accent)]"
                />
                {p.name}
              </label>
            ))}
          </div>
        )}
        <label className="flex items-center gap-2 mb-4 text-[13px] text-muted cursor-pointer">
          <input
            type="checkbox"
            checked={saveDefault}
            onChange={(e: ChangeEvent<HTMLInputElement>) => setSaveDefault(e.currentTarget.checked)}
            className="accent-[var(--accent)]"
          />
          Save as default
        </label>
        <div className="flex justify-end gap-2">
          <Btn type="button" onClick={oncancel}>
            Cancel
          </Btn>
          <Btn type="submit" variant="cta" disabled={!canSubmit}>
            Create
          </Btn>
        </div>
      </form>
    </BaseDialog>
  );
}
