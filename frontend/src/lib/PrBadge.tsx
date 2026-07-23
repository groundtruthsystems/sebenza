import type { PrEntry } from "./types";
import { prBadgeClass, prLabel } from "./utils";

export default function PrBadge({
  pr,
  clickable = false,
}: {
  pr: PrEntry;
  clickable?: boolean;
}) {
  const label = prLabel(pr);
  if (clickable && pr.url) {
    return (
      <a
        href={pr.url}
        target="_blank"
        rel="noopener"
        className={`shrink-0 text-[10px] font-medium px-1.5 py-0.5 rounded-full no-underline hover:opacity-80 ${prBadgeClass(pr.state)}`}
      >
        {label}
      </a>
    );
  }
  return (
    <span
      className={`shrink-0 text-[10px] font-medium px-1.5 py-0.5 rounded-full ${prBadgeClass(pr.state)}`}
    >
      {label}
    </span>
  );
}
