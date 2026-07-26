import { useMemo, useState } from "react";
import type { CiCheck, PrEntry } from "./types";
import { api } from "./api";
import { normalizeTextForPrompt } from "./promptUtils";
import { prLabel, errorMessage } from "./utils";
import { useStore } from "../store";
import BaseDialog from "./BaseDialog";
import Btn from "./Btn";
import LinkBtn from "./LinkBtn";

export default function CiDetailsDialog({
  pr,
  branch,
  onclose,
  onfixsuccess,
}: {
  pr: PrEntry;
  branch: string;
  onclose: () => void;
  onfixsuccess: () => void;
}) {
  const [logsByRunId, setLogsByRunId] = useState<Map<number, string>>(new Map());
  const [expandedChecks, setExpandedChecks] = useState<Set<string>>(new Set());
  const [loadingRunId, setLoadingRunId] = useState<number | null>(null);
  const [logsError, setLogsError] = useState("");
  const [fixLoading, setFixLoading] = useState(false);
  const [fixError, setFixError] = useState("");
  const success = useStore((s) => s.success);

  const label = useMemo(() => prLabel(pr), [pr]);

  function checkKey(check: { name: string; runId: number | null }): string {
    return `${check.name}:${check.runId}`;
  }

  function logsForCheck(check: { name: string; runId: number | null }): string {
    if (check.runId === null) return "";
    const allLogs = logsByRunId.get(check.runId);
    if (!allLogs) return "";
    const prefix = check.name + "\t";
    return allLogs
      .split("\n")
      .filter((line) => line.startsWith(prefix))
      .map((line) => line.slice(prefix.length))
      .join("\n");
  }

  function toggleCheck(key: string): void {
    setExpandedChecks((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  }

  async function handleViewLogs(check: { runId: number; name: string }): Promise<void> {
    const key = checkKey(check);
    if (logsByRunId.has(check.runId)) {
      toggleCheck(key);
      return;
    }
    setExpandedChecks((prev) => new Set(prev).add(key));
    setLogsError("");
    setLoadingRunId(check.runId);
    try {
      const { logs } = await api.fetchCiLogs({ params: { runId: check.runId } });
      setLogsByRunId((prev) => new Map(prev).set(check.runId, logs));
    } catch (err) {
      setLogsError(errorMessage(err));
      setExpandedChecks((prev) => {
        const next = new Set(prev);
        next.delete(key);
        return next;
      });
    } finally {
      setLoadingRunId(null);
    }
  }

  async function handleFix(checkName: string, filteredLogs: string): Promise<void> {
    if (!branch) return;
    setFixError("");
    setFixLoading(true);
    const preamble =
      ["Fix the failing CI check.", `PR: ${label}`, `Check: ${checkName}`, "", "Logs:"].join("\n") +
      "\n";
    const sanitizedLogs = normalizeTextForPrompt(filteredLogs);
    try {
      await api.sendWorktreePrompt({
        params: { name: branch },
        body: { text: sanitizedLogs, preamble },
      });
      success(`Asked agent to fix ${checkName}`);
      onfixsuccess();
    } catch (err) {
      setFixError(errorMessage(err));
    } finally {
      setFixLoading(false);
    }
  }

  async function handleCopy(filteredLogs: string): Promise<void> {
    await navigator.clipboard.writeText(filteredLogs);
    success("Copied logs");
  }

  function statusIcon(status: string): string {
    if (status === "success") return "✓";
    if (status === "failed") return "✗";
    if (status === "skipped") return "—";
    return "○";
  }

  function statusColor(status: string): string {
    if (status === "success") return "text-success";
    if (status === "failed") return "text-danger";
    if (status === "pending") return "text-warning";
    return "text-muted";
  }

  return (
    <BaseDialog onclose={onclose} wide>
      <h2 className="text-base mb-4">CI Checks &mdash; {label}</h2>

      <ul className="list-none p-0 m-0 flex flex-col gap-2 mb-4">
        {pr.ciChecks.map((check: CiCheck) => {
          const key = checkKey(check);
          const cached = check.runId !== null && logsByRunId.has(check.runId);
          const expanded = expandedChecks.has(key);
          const filtered = expanded ? logsForCheck(check) : "";
          return (
            <li key={check.name + check.runId} className="rounded-md border border-edge bg-surface p-3">
              <div className="flex items-center gap-2">
                <span className={`text-sm font-bold ${statusColor(check.status)}`}>
                  {statusIcon(check.status)}
                </span>
                <span className="text-[13px] font-medium flex-1 truncate">{check.name}</span>
                <span className={`text-[11px] ${statusColor(check.status)}`}>{check.status}</span>
              </div>
              <div className="flex items-center gap-2 mt-1.5">
                {check.status === "failed" && check.runId !== null && (
                  cached ? (
                    <LinkBtn onClick={() => toggleCheck(key)}>
                      {expanded ? "Hide logs" : "Show logs"}
                    </LinkBtn>
                  ) : (
                    <LinkBtn onClick={() => handleViewLogs({ runId: check.runId!, name: check.name })}>
                      View logs
                    </LinkBtn>
                  )
                )}
                {check.url && (
                  <a
                    href={check.url}
                    target="_blank"
                    rel="noopener"
                    className="text-[11px] text-muted hover:text-primary no-underline hover:underline"
                  >
                    GitHub &nearr;
                  </a>
                )}
              </div>

              {check.runId !== null && loadingRunId === check.runId && expanded ? (
                <div className="text-[12px] text-muted py-2 mt-2">Loading logs...</div>
              ) : (
                expanded &&
                filtered && (
                  <div className="mt-2">
                    <pre className="bg-surface border border-edge rounded-md p-3 text-[11px] font-mono overflow-x-auto max-h-[300px] overflow-y-auto whitespace-pre-wrap m-0">
                      {filtered}
                    </pre>
                    <div className="flex justify-end items-center gap-2 mt-1.5">
                      <LinkBtn onClick={() => handleCopy(filtered)}>Copy logs</LinkBtn>
                      <Btn
                        variant="cta"
                        small
                        disabled={!branch || fixLoading}
                        onClick={() => handleFix(check.name, filtered)}
                      >
                        {fixLoading ? "Asking agent..." : "Ask agent to fix"}
                      </Btn>
                    </div>
                  </div>
                )
              )}
              {logsError &&
                loadingRunId === null &&
                check.runId !== null &&
                !logsByRunId.has(check.runId) && (
                  <div className="text-[12px] text-danger py-2 mt-2">{logsError}</div>
                )}
              {fixError && <div className="text-[12px] text-danger py-1.5">{fixError}</div>}
            </li>
          );
        })}
      </ul>

      <div className="flex justify-end">
        <Btn type="button" onClick={onclose}>
          Close
        </Btn>
      </div>
    </BaseDialog>
  );
}
