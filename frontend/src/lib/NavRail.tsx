/**
 * The icon-only vertical rail down the left edge.
 *
 * Sebenza's top-level destinations are split between two mechanisms: worktrees
 * and tracks are view state inside a project, while the inbox and the registry
 * are their own pages outside any project prefix. The rail hides that seam —
 * every destination is one click, whether it flips state or navigates.
 */

export type NavDestination = "worktrees" | "tracks" | "inbox" | "registry";

interface NavRailProps {
  active: NavDestination;
  /** Where the project-scoped destinations live, e.g. `/my-app`. Empty when
   *  no project is open, which hides them rather than linking nowhere. */
  projectBase?: string;
  /** Switch view in-place. Provided only by the project dashboard, which owns
   *  that state; elsewhere the rail navigates instead. */
  onSelectView?: (view: "terminal" | "tracks") => void;
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
    case "tracks":
      return (
        <svg {...common}>
          <rect x="3" y="4" width="5" height="12" rx="1" />
          <rect x="9.5" y="4" width="5" height="16" rx="1" />
          <rect x="16" y="4" width="5" height="8" rx="1" />
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
  worktrees: "Worktrees",
  tracks: "Tracks",
  inbox: "Inbox",
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

  // In the dashboard these flip view state; from another page they have to
  // navigate into the project first.
  const viewItem = (dest: "worktrees" | "tracks") => {
    const view = dest === "tracks" ? "tracks" : "terminal";
    if (onSelectView) {
      return (
        <button
          type="button"
          className={cls(dest)}
          title={LABELS[dest]}
          aria-label={LABELS[dest]}
          aria-current={active === dest ? "page" : undefined}
          onClick={() => onSelectView(view)}
        >
          <Icon name={dest} />
        </button>
      );
    }
    if (!projectBase) return null;
    return (
      <a
        href={`${projectBase}/?view=${view}`}
        className={cls(dest)}
        title={LABELS[dest]}
        aria-label={LABELS[dest]}
      >
        <Icon name={dest} />
      </a>
    );
  };

  return (
    <nav className="nav-rail" aria-label="Sections">
      {viewItem("worktrees")}
      {viewItem("tracks")}
      <div className="nav-rail-spacer" />
      <a
        href="/inbox"
        className={cls("inbox")}
        title={LABELS.inbox}
        aria-label={LABELS.inbox}
        aria-current={active === "inbox" ? "page" : undefined}
      >
        <Icon name="inbox" />
      </a>
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
