import "./TabBar.css";
import { useEffect, useState } from "react";
import type { WorktreeTab } from "./types";

export default function TabBar({
  tabs,
  activeTabId,
  busy = false,
  canFork = false,
  oncreate,
  oncreateshell,
  onselect,
  ondelete,
}: {
  tabs: WorktreeTab[];
  activeTabId: string | null;
  busy?: boolean;
  canFork?: boolean;
  oncreate: () => void;
  oncreateshell: () => void;
  onselect: (tabId: string) => void;
  ondelete: (tabId: string) => void;
}) {
  const [addMenuOpen, setAddMenuOpen] = useState(false);

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

  function chooseFork(): void {
    setAddMenuOpen(false);
    oncreate();
  }

  function chooseTerminal(): void {
    setAddMenuOpen(false);
    oncreateshell();
  }

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
            {(tab.kind === "fork" || tab.kind === "shell") && (
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
      <div className="tab-add-container relative flex items-center shrink-0 border-l border-edge">
        <button
          type="button"
          aria-label="New tab"
          title={canFork ? "New tab (fork or terminal)" : "New terminal tab"}
          className="px-3 py-2 text-sm text-muted cursor-pointer border-none bg-transparent hover:text-accent disabled:opacity-50 disabled:cursor-not-allowed"
          disabled={busy}
          onClick={() => {
            // Fork-capable agents choose fork vs terminal; others get a terminal directly.
            if (canFork) setAddMenuOpen((open) => !open);
            else chooseTerminal();
          }}
        >
          +
        </button>
        {addMenuOpen && (
          <div className="absolute right-0 top-full z-20 min-w-[130px] bg-sidebar border border-edge rounded-md shadow-lg overflow-hidden">
            <button
              type="button"
              className="w-full px-3 py-2 text-left text-sm text-primary bg-transparent border-none cursor-pointer hover:bg-hover disabled:opacity-50 disabled:cursor-not-allowed"
              disabled={busy}
              onClick={chooseFork}
            >
              Fork
            </button>
            <button
              type="button"
              className="w-full px-3 py-2 text-left text-sm text-primary bg-transparent border-none cursor-pointer hover:bg-hover disabled:opacity-50 disabled:cursor-not-allowed"
              disabled={busy}
              onClick={chooseTerminal}
            >
              Terminal
            </button>
          </div>
        )}
      </div>
    </nav>
  );
}
