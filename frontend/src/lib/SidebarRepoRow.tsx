export default function SidebarRepoRow({
  label,
  onpull,
}: {
  label: string;
  onpull: () => void;
}) {
  return (
    <div className="shrink-0 border-t border-edge px-3 py-2 flex items-center gap-2">
      <span className="text-[11px] text-muted font-medium truncate flex-1">{label}</span>
      <button
        type="button"
        className="shrink-0 text-[9px] px-1.5 py-0.5 rounded border border-edge text-muted font-medium cursor-pointer hover:bg-hover hover:text-primary"
        title="Pull latest from remote"
        onClick={onpull}
      >
        Pull
      </button>
    </div>
  );
}
