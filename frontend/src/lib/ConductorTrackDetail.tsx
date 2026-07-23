import { useEffect, useState, type ReactNode } from "react";
import type { ConductorPlan, ConductorPlanPhase, ConductorTrack } from "./types";
import { fetchConductorFile } from "./api";
import { errorMessage } from "./utils";
import BaseDialog from "./BaseDialog";
import ConductorMarkdown from "./ConductorMarkdown";
import { statusDotClass, statusLabel, statusTextClass } from "./conductorStatus";

type DetailTab = "plan" | "spec" | "design";

function TabBtn({
  active,
  disabled,
  onClick,
  children,
}: {
  active: boolean;
  disabled: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={`px-3 py-1 text-[13px] rounded-md border transition-colors disabled:opacity-40 disabled:cursor-not-allowed ${
        active
          ? "border-accent bg-active text-primary"
          : "border-edge text-muted hover:text-primary hover:bg-hover"
      }`}
    >
      {children}
    </button>
  );
}

function PhaseBlock({ phase }: { phase: ConductorPlanPhase }) {
  return (
    <div className="rounded-lg border border-edge bg-surface">
      <div className="flex items-center gap-2 px-3 py-2 border-b border-edge">
        <span className={`w-2 h-2 rounded-full shrink-0 ${statusDotClass(phase.status)}`} />
        <span className="text-[13px] text-primary">{phase.name}</span>
        <span className={`ml-auto text-[10px] ${statusTextClass(phase.status)}`}>
          {statusLabel(phase.status)}
        </span>
      </div>
      <div className="p-2 space-y-1">
        {(phase.tasks ?? []).map((task) => (
          <details key={task.id} className="rounded-md">
            <summary className="flex items-center gap-2 px-2 py-1 cursor-pointer text-[12px]">
              <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${statusDotClass(task.status)}`} />
              <span className="text-primary">{task.name}</span>
            </summary>
            <div className="pl-5 pr-2 pb-2 pt-1 text-[12px] text-muted space-y-1">
              {task.description && <p>{task.description}</p>}
              {task.blocked_reason && <p className="text-danger">Blocked: {task.blocked_reason}</p>}
              {task.commit && <p className="font-mono text-[11px]">commit {task.commit}</p>}
              {task.notes && <p className="italic">{task.notes}</p>}
              {(task.subtasks ?? []).length > 0 && (
                <ul className="mt-1 space-y-0.5">
                  {task.subtasks!.map((st) => (
                    <li key={st.id} className="flex items-center gap-2">
                      <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${statusDotClass(st.status)}`} />
                      <span>{st.name}</span>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </details>
        ))}
      </div>
    </div>
  );
}

function PlanView({ branch, path }: { branch: string; path: string }) {
  const [plan, setPlan] = useState<ConductorPlan | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError("");
    fetchConductorFile(branch, path)
      .then((res) => {
        if (cancelled) return;
        try {
          setPlan(JSON.parse(res.content) as ConductorPlan);
        } catch {
          setError("Could not parse plan.json");
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(errorMessage(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [branch, path]);

  if (loading) return <div className="text-sm text-muted py-8 text-center">Loading plan…</div>;
  if (error) return <div className="text-sm text-danger py-8 text-center">{error}</div>;
  if (!plan || plan.phases.length === 0)
    return <div className="text-sm text-muted py-8 text-center">No phases</div>;

  return (
    <div className="space-y-3">
      {plan.phases.map((phase) => (
        <PhaseBlock key={phase.id} phase={phase} />
      ))}
    </div>
  );
}

export default function ConductorTrackDetail({
  branch,
  track,
  onclose,
}: {
  branch: string;
  track: ConductorTrack;
  onclose: () => void;
}) {
  const hasPlan = !!track.plan_path;
  const hasSpec = !!track.spec_path;
  const hasDesign = !!track.design_path;
  const [tab, setTab] = useState<DetailTab>(hasPlan ? "plan" : hasSpec ? "spec" : "design");

  return (
    <BaseDialog onclose={onclose} wide maxWidth="90vw">
      <div className="mb-4">
        <div className="flex flex-wrap items-center gap-2">
          {track.type && (
            <span className="text-[10px] uppercase px-1.5 py-0.5 rounded border border-edge text-muted">
              {track.type}
            </span>
          )}
          <span className={`text-[11px] ${statusTextClass(track.status)}`}>
            {statusLabel(track.status)}
          </span>
          <span className="text-[11px] text-muted">
            {track.progress.completed_tasks}/{track.progress.total_tasks} (
            {Math.round(track.progress.percentage)}%)
          </span>
        </div>
        <h2 className="text-base mt-1">{track.description}</h2>
        {track.blocked_reason && (
          <p className="text-[12px] text-danger mt-1">Blocked: {track.blocked_reason}</p>
        )}
      </div>

      <div className="flex gap-1 mb-3">
        <TabBtn active={tab === "plan"} disabled={!hasPlan} onClick={() => setTab("plan")}>
          Plan
        </TabBtn>
        <TabBtn active={tab === "spec"} disabled={!hasSpec} onClick={() => setTab("spec")}>
          Spec
        </TabBtn>
        <TabBtn active={tab === "design"} disabled={!hasDesign} onClick={() => setTab("design")}>
          Design
        </TabBtn>
      </div>

      <div className="overflow-auto max-h-[70vh]">
        {tab === "plan" && hasPlan && <PlanView branch={branch} path={track.plan_path!} />}
        {tab === "spec" && hasSpec && <ConductorMarkdown branch={branch} path={track.spec_path!} />}
        {tab === "design" && hasDesign && (
          <ConductorMarkdown branch={branch} path={track.design_path!} />
        )}
      </div>
    </BaseDialog>
  );
}
