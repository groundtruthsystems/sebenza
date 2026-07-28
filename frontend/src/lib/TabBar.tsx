import "./TabBar.css";
import { useEffect, useRef, useState } from "react";
import type { AgentSummary, WorktreeTab } from "./types";

export default function TabBar({
  tabs,
  activeTabId,
  agents = [],
  busy = false,
  canFork = false,
  oncreate,
  oncreateshell,
  oncreateagent,
  onselect,
  ondelete,
}: {
  tabs: WorktreeTab[];
  activeTabId: string | null;
  /** Configured agents, built-in and custom, offered under "New session". */
  agents?: AgentSummary[];
  busy?: boolean;
  canFork?: boolean;
  oncreate: () => void;
  oncreateshell: () => void;
  oncreateagent: (agentId: string) => void;
  onselect: (tabId: string) => void;
  ondelete: (tabId: string) => void;
}) {
  const [addMenuOpen, setAddMenuOpen] = useState(false);
  const [sessionMenuOpen, setSessionMenuOpen] = useState(false);
  const addButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!addMenuOpen) return;
    function handleClickOutside(e: MouseEvent): void {
      if (e.target instanceof Element && !e.target.closest(".tab-add-container")) {
        setAddMenuOpen(false);
      }
    }
    window.addEventListener("click", handleClickOutside);
    return () => window.removeEventListener("click", handleClickOutside);
  }, [addMenuOpen]);

  // Collapsing the outer menu must not leave the nested list expanded for the
  // next open.
  useEffect(() => {
    if (!addMenuOpen) setSessionMenuOpen(false);
  }, [addMenuOpen]);

  function closeMenu(): void {
    setAddMenuOpen(false);
    setSessionMenuOpen(false);
  }

  function chooseFork(): void {
    closeMenu();
    oncreate();
  }

  function chooseTerminal(): void {
    closeMenu();
    oncreateshell();
  }

  function chooseAgent(agentId: string): void {
    closeMenu();
    oncreateagent(agentId);
  }

  function handleMenuKeyDown(e: React.KeyboardEvent): void {
    if (e.key !== "Escape") return;
    e.stopPropagation();
    // Escape backs out one level at a time, then returns focus to the "+".
    if (sessionMenuOpen) {
      setSessionMenuOpen(false);
      return;
    }
    setAddMenuOpen(false);
    addButtonRef.current?.focus();
  }

  const menuItemClass =
    "w-full px-3 py-2 text-left text-sm text-primary bg-transparent border-none cursor-pointer hover:bg-hover disabled:opacity-50 disabled:cursor-not-allowed";

  return (
    <nav className="flex items-stretch bg-topbar border-b border-edge tab-bar">
      {/* Tabs scroll within their own area so the "+" stays pinned + visible. */}
      <div className="flex items-stretch overflow-x-auto min-w-0 flex-1">
        {tabs.map((tab) => (
          <div
            key={tab.tabId}
            className={`flex items-center border-r border-edge ${activeTabId === tab.tabId ? "tab-active" : ""}`}
          >
            <button
              type="button"
              className={`px-3 py-2 text-sm font-medium whitespace-nowrap cursor-pointer border-none bg-transparent ${activeTabId === tab.tabId ? "text-accent" : "text-muted hover:text-accent"}`}
              onClick={() => onselect(tab.tabId)}
            >
              {tab.label}
            </button>
            {/* Every tab but the root can be closed. */}
            {tab.kind !== "root" && (
              <button
                type="button"
                aria-label={`Close ${tab.label}`}
                className="mr-1.5 flex items-center justify-center w-5 h-5 rounded text-muted cursor-pointer border-none bg-transparent hover:text-danger hover:bg-hover disabled:opacity-50 disabled:cursor-not-allowed"
                disabled={busy}
                onClick={() => ondelete(tab.tabId)}
              >
                ×
              </button>
            )}
          </div>
        ))}
      </div>
      <div
        className="tab-add-container relative flex items-center shrink-0 border-l border-edge"
        onKeyDown={handleMenuKeyDown}
      >
        <button
          ref={addButtonRef}
          type="button"
          aria-label="New tab"
          aria-expanded={addMenuOpen}
          title="New tab"
          className="px-3 py-2 text-sm text-muted cursor-pointer border-none bg-transparent hover:text-accent disabled:opacity-50 disabled:cursor-not-allowed"
          disabled={busy}
          onClick={() => setAddMenuOpen((open) => !open)}
        >
          +
        </button>
        {addMenuOpen && (
          <div className="absolute right-0 top-full z-20 min-w-[170px] bg-sidebar border border-edge rounded-md shadow-lg overflow-hidden">
            {/* Forking continues the root conversation, so it needs an agent
                whose sessions can be forked. */}
            {canFork && (
              <button type="button" className={menuItemClass} disabled={busy} onClick={chooseFork}>
                Fork
              </button>
            )}
            <button type="button" className={menuItemClass} disabled={busy} onClick={chooseTerminal}>
              Terminal
            </button>
            {agents.length > 0 && (
              <>
                <div className="h-px bg-edge" role="separator" />
                <button
                  type="button"
                  className={`${menuItemClass} flex items-center justify-between gap-2`}
                  aria-expanded={sessionMenuOpen}
                  disabled={busy}
                  onClick={() => setSessionMenuOpen((open) => !open)}
                >
                  <span>New session</span>
                  <span aria-hidden="true" className="text-muted text-xs">
                    {sessionMenuOpen ? "▾" : "▸"}
                  </span>
                </button>
                {sessionMenuOpen && (
                  <ul className="list-none m-0 p-0 max-h-64 overflow-y-auto border-t border-edge">
                    {agents.map((agent) => (
                      <li key={agent.id}>
                        <button
                          type="button"
                          className={`${menuItemClass} pl-6`}
                          disabled={busy}
                          onClick={() => chooseAgent(agent.id)}
                        >
                          {agent.label}
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </>
            )}
          </div>
        )}
      </div>
    </nav>
  );
}
