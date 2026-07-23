import "./PaneBar.css";

export default function PaneBar({
  activePane,
  panes,
  onselect,
}: {
  activePane: number;
  panes: { index: number; label: string }[];
  onselect: (pane: number) => void;
}) {
  return (
    <nav className="flex items-stretch bg-topbar border-t border-edge pane-bar">
      {panes.map((p) => (
        <button
          key={p.index}
          type="button"
          className={`flex-1 py-3 text-sm font-medium cursor-pointer border-none bg-transparent ${activePane === p.index ? "text-accent pane-active" : "text-muted"}`}
          onClick={() => onselect(p.index)}
        >
          {p.label}
        </button>
      ))}
    </nav>
  );
}
