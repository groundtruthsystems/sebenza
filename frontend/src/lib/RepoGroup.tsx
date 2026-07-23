import type { PrEntry, ServiceStatus } from "./types";
import PrStatusGroup from "./PrStatusGroup";

export default function RepoGroup({
  label,
  prs,
  services = [],
  onCiClick,
  onReviewsClick,
}: {
  label?: string;
  prs: PrEntry[];
  services?: ServiceStatus[];
  onCiClick: (pr: PrEntry) => void;
  onReviewsClick: (pr: PrEntry) => void;
}) {
  return (
    <div className="repo-group flex flex-wrap items-center gap-x-2 gap-y-1.5 min-w-0">
      {label && <span className="shrink-0 text-[10px] font-medium text-muted">{label}:</span>}
      {prs.map((pr) => (
        <PrStatusGroup
          key={`${pr.repo}#${pr.number}`}
          pr={pr}
          onCiClick={onCiClick}
          onReviewsClick={onReviewsClick}
        />
      ))}
      {services.map(
        (svc) =>
          svc.port && (
            <a
              key={`${svc.name}:${svc.port}`}
              href={`${window.location.protocol}//${window.location.hostname}:${svc.port}`}
              target="_blank"
              rel="noopener"
              className={`shrink-0 text-[11px] px-1.5 py-0.5 rounded border font-mono no-underline hover:opacity-80 ${
                svc.running
                  ? "text-success border-success/40"
                  : "text-muted border-edge pointer-events-none"
              }`}
            >
              {svc.name} :{svc.port}
            </a>
          ),
      )}
    </div>
  );
}
