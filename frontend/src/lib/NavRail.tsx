/**
 * The icon-only vertical rail down the left edge.
 *
 * The inbox leads: it is where work starts, before a worktree exists. Below it
 * sits the project's worktrees, and the registry sits at the foot as the
 * cross-project view.
 *
 * Tracks is deliberately absent — it is a view *of* a worktree, reached from
 * the top bar's toggle, not a top-level destination.
 */

export type NavDestination = "inbox" | "worktrees" | "registry";

interface NavRailProps {
  active: NavDestination;
  /** Where the project-scoped destinations live, e.g. `/my-app`. Empty when
   *  no project is open, which hides them rather than linking nowhere. */
  projectBase?: string;
  /** Return to the terminal view in place. Provided only by the project
   *  dashboard, which owns that state; elsewhere the rail navigates instead. */
  onSelectView?: (view: "terminal") => void;
}

function Icon({ name }: { name: NavDestination }) {
  const common = {
    width: 20,
    height: 20,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.7,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    "aria-hidden": true,
  };
  switch (name) {
    case "inbox":
      return (
        <svg {...common}>
          <path d="M3 13h4l1.5 3h7L17 13h4" />
          <path d="M4.5 5.5 3 13v5a1 1 0 0 0 1 1h16a1 1 0 0 0 1-1v-5l-1.5-7.5A1 1 0 0 0 18.5 5h-13a1 1 0 0 0-1 .5z" />
        </svg>
      );
    case "worktrees":
      return (
        <svg {...common}>
          <rect x="3" y="4" width="18" height="16" rx="2" />
          <path d="M7 9h4M7 13h7" />
        </svg>
      );
    case "registry":
      return (
        <svg {...common}>
          <circle cx="12" cy="12" r="9" />
          <path d="M3 12h18M12 3c2.5 2.7 2.5 15.3 0 18M12 3c-2.5 2.7-2.5 15.3 0 18" />
        </svg>
      );
  }
}

const LABELS: Record<NavDestination, string> = {
  inbox: "Inbox",
  worktrees: "Worktrees",
  registry: "Registry",
};

export default function NavRail({
  active,
  projectBase = "",
  onSelectView,
}: NavRailProps) {
  const cls = (dest: NavDestination) =>
    [
      "nav-rail-btn",
      active === dest ? "nav-rail-active" : "",
    ]
      .filter(Boolean)
      .join(" ");

  // In the dashboard this returns to the terminal view; from another page it
  // has to navigate into the project first.
  const worktreesItem = () => {
    if (onSelectView) {
      return (
        <button
          type="button"
          className={cls("worktrees")}
          title={LABELS.worktrees}
          aria-label={LABELS.worktrees}
          aria-current={active === "worktrees" ? "page" : undefined}
          onClick={() => onSelectView("terminal")}
        >
          <Icon name="worktrees" />
        </button>
      );
    }
    if (!projectBase) return null;
    return (
      <a
        href={`${projectBase}/`}
        className={cls("worktrees")}
        title={LABELS.worktrees}
        aria-label={LABELS.worktrees}
      >
        <Icon name="worktrees" />
      </a>
    );
  };

  return (
    <nav className="nav-rail" aria-label="Sections">
      <a
        href="/inbox"
        className={cls("inbox")}
        title={LABELS.inbox}
        aria-label={LABELS.inbox}
        aria-current={active === "inbox" ? "page" : undefined}
      >
        <Icon name="inbox" />
      </a>
      {worktreesItem()}
      <div className="nav-rail-spacer" />
      <a
        href="/registry"
        className={cls("registry")}
        title={LABELS.registry}
        aria-label={LABELS.registry}
        aria-current={active === "registry" ? "page" : undefined}
      >
        <Icon name="registry" />
      </a>
    </nav>
  );
}
