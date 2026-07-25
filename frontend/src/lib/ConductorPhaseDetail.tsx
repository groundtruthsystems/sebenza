import { useEffect, useState } from "react";
import type { ConductorPlan, ConductorPlanPhase, ConductorPlanTask } from "./types";
import { fetchConductorFile } from "./api";
import { errorMessage } from "./utils";
import BaseDialog from "./BaseDialog";
import { statusDotClass, statusLabel, statusTextClass } from "./conductorStatus";

function TaskItem({ task }: { task: ConductorPlanTask }) {
  return (
    <details className="rounded-md">
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
  );
}

/** Modal showing a single phase's tasks → subtasks, read from the track's
 *  `plan.json` and matched by phase id. */
export default function ConductorPhaseDetail({
  branch,
  planPath,
  phaseId,
  phaseName,
  onclose,
}: {
  branch: string;
  planPath: string | undefined;
  phaseId: string;
  phaseName: string;
  onclose: () => void;
}) {
  const [phase, setPhase] = useState<ConductorPlanPhase | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!planPath) {
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError("");
    fetchConductorFile(branch, planPath)
      .then((res) => {
        if (cancelled) return;
        try {
          const plan = JSON.parse(res.content) as ConductorPlan;
          setPhase(plan.phases.find((p) => p.id === phaseId) ?? null);
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
  }, [branch, planPath, phaseId]);

  return (
    <BaseDialog onclose={onclose} wide maxWidth="80vw">
      <div className="mb-3 flex items-center gap-2">
        {phase && (
          <span className={`w-2 h-2 rounded-full shrink-0 ${statusDotClass(phase.status)}`} />
        )}
        <h2 className="text-base">{phaseName}</h2>
        {phase && (
          <span className={`text-[11px] ${statusTextClass(phase.status)}`}>
            {statusLabel(phase.status)}
          </span>
        )}
      </div>
      <div className="overflow-auto max-h-[70vh]">
        {loading ? (
          <div className="text-sm text-muted py-8 text-center">Loading plan…</div>
        ) : error ? (
          <div className="text-sm text-danger py-8 text-center">{error}</div>
        ) : !phase ? (
          <div className="text-sm text-muted py-8 text-center">No plan detail for this phase.</div>
        ) : (phase.tasks ?? []).length === 0 ? (
          <div className="text-sm text-muted py-8 text-center">No tasks</div>
        ) : (
          <div className="space-y-1">
            {phase.tasks!.map((task) => (
              <TaskItem key={task.id} task={task} />
            ))}
          </div>
        )}
      </div>
    </BaseDialog>
  );
}
