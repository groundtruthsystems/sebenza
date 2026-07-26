import { useEffect, useMemo, useRef, useState, type FocusEvent, type KeyboardEvent, type MouseEvent } from "react";
import type { AvailableBranch } from "./types";
import Toggle from "./Toggle";
import { searchMatch } from "./utils";

export default function BranchSelector({
  label,
  selected = "",
  branches = [],
  loading = false,
  error = null,
  placeholder = "Select a branch",
  initialOpen = false,
  disabled = false,
  inlineToggleLabel,
  inlineToggleAriaLabel,
  inlineToggleChecked = false,
  oninlinetoggle,
  onselect,
}: {
  label: string;
  selected?: string;
  branches?: AvailableBranch[];
  loading?: boolean;
  error?: string | null;
  placeholder?: string;
  initialOpen?: boolean;
  disabled?: boolean;
  inlineToggleLabel?: string;
  inlineToggleAriaLabel?: string;
  inlineToggleChecked?: boolean;
  oninlinetoggle?: () => void;
  onselect: (branch: string) => void;
}) {
  const [selectorOpen, setSelectorOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const fieldEl = useRef<HTMLDivElement>(null);
  const searchEl = useRef<HTMLInputElement>(null);
  const autoOpened = useRef(false);
  const autoFocused = useRef(false);

  const filteredBranches = useMemo(
    () =>
      searchQuery.trim()
        ? branches.filter((branch) => searchMatch(searchQuery, branch.name))
        : branches,
    [searchQuery, branches],
  );

  useEffect(() => {
    if (!initialOpen || autoOpened.current) return;
    autoOpened.current = true;
    setSelectorOpen(true);
  }, [initialOpen]);

  useEffect(() => {
    if (!selectorOpen || autoFocused.current) return;
    autoFocused.current = true;
    focusSearch();
  }, [selectorOpen]);

  function focusSearch(): void {
    queueMicrotask(() => searchEl.current?.focus());
  }

  function closeSelector(): void {
    setSelectorOpen(false);
    setSearchQuery("");
    autoFocused.current = false;
  }

  function toggleSelector(): void {
    if (selectorOpen) {
      closeSelector();
      return;
    }
    setSelectorOpen(true);
  }

  function selectBranch(name: string): void {
    onselect(name);
    closeSelector();
  }

  function handleFocusOut(event: FocusEvent): void {
    const nextTarget = event.relatedTarget;
    if (nextTarget instanceof Node && fieldEl.current?.contains(nextTarget)) {
      return;
    }
    closeSelector();
  }

  function toggleInlineControl(): void {
    oninlinetoggle?.();
  }

  return (
    <div ref={fieldEl} onBlur={handleFocusOut}>
      <span className="block text-xs text-muted mb-1.5">{label}</span>
      <button
        type="button"
        disabled={disabled}
        className="flex w-full items-center justify-between gap-3 rounded-md border border-edge bg-surface px-2.5 py-1.5 text-left text-[13px] text-primary outline-none transition-colors hover:bg-hover focus:border-accent disabled:cursor-not-allowed disabled:opacity-60 disabled:hover:bg-surface"
        aria-label={label}
        aria-expanded={disabled ? undefined : selectorOpen}
        onClick={toggleSelector}
      >
        <span className={selected ? "font-mono" : "text-muted/50"}>
          {selected || placeholder}
        </span>
        {!disabled && (
          <span className="text-[11px] text-muted">{selectorOpen ? "▴" : "▾"}</span>
        )}
      </button>
      {selectorOpen && !disabled && (
        <div className="mt-2 rounded-lg border border-edge bg-surface/60">
          <div className="border-b border-edge p-2">
            <input
              ref={searchEl}
              type="text"
              className="w-full rounded-md border border-edge bg-surface px-2.5 py-1.5 text-[12px] text-primary placeholder:text-muted/50 outline-none focus:border-accent"
              aria-label={`${label} search`}
              placeholder="Search branches..."
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.currentTarget.value)}
              onKeyDown={(event: KeyboardEvent) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  if (filteredBranches[0]) {
                    selectBranch(filteredBranches[0].name);
                  }
                }
                if (event.key === "Escape") {
                  event.preventDefault();
                  closeSelector();
                }
              }}
            />
          </div>
          <div
            onMouseDown={(event: MouseEvent) => {
              if (oninlinetoggle) event.preventDefault();
            }}
            className="border-b border-edge px-3 py-2 text-[11px] text-muted flex items-center justify-between gap-3"
          >
            <div className="min-w-0 flex items-center gap-2">
              {loading && filteredBranches.length === 0 ? (
                <span>Loading...</span>
              ) : error && filteredBranches.length === 0 ? (
                <span>Load failed</span>
              ) : (
                <span>
                  {filteredBranches.length !== branches.length
                    ? `${filteredBranches.length}/${branches.length}`
                    : branches.length}
                  {" "}available
                </span>
              )}
              {loading && filteredBranches.length > 0 ? (
                <span className="shrink-0 text-[10px] text-warning">Updating...</span>
              ) : error && filteredBranches.length > 0 ? (
                <span className="shrink-0 text-[10px] text-danger">Update failed</span>
              ) : null}
            </div>
            {inlineToggleLabel && oninlinetoggle && (
              <div className="flex items-center gap-1.5 shrink-0">
                <button
                  type="button"
                  className="text-[10px] text-muted hover:text-primary transition-colors"
                  onMouseDown={(event: MouseEvent) => event.preventDefault()}
                  onClick={toggleInlineControl}
                >
                  {inlineToggleLabel}
                </button>
                <Toggle
                  checked={inlineToggleChecked}
                  size="sm"
                  preventMouseFocus={true}
                  aria-label={inlineToggleAriaLabel ?? inlineToggleLabel}
                  onToggle={toggleInlineControl}
                />
              </div>
            )}
          </div>
          {loading && filteredBranches.length === 0 ? (
            <p className="px-3 py-2 text-xs text-muted">Loading branches...</p>
          ) : error && filteredBranches.length === 0 ? (
            <p className="px-3 py-2 text-xs text-muted">Failed to load branches: {error}</p>
          ) : filteredBranches.length === 0 ? (
            <p className="px-3 py-2 text-xs text-muted">No matching branches</p>
          ) : (
            <ul className="max-h-48 overflow-y-auto py-1">
              {filteredBranches.map((branch) => (
                <li key={branch.name}>
                  <button
                    type="button"
                    className={`flex w-full items-center justify-between gap-3 px-3 py-2 text-left text-[12px] transition-colors hover:bg-hover
                  ${selected === branch.name ? "bg-accent/10" : ""}`}
                    onMouseDown={(e: MouseEvent) => e.preventDefault()}
                    onClick={() => selectBranch(branch.name)}
                  >
                    <span className="font-mono text-primary">{branch.name}</span>
                    {selected === branch.name && (
                      <span className="text-[10px] text-accent">Selected</span>
                    )}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
