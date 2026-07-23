import { useEffect, useMemo, useRef, useState } from "react";
import { html as diff2html } from "diff2html";
import { ColorSchemeType } from "diff2html/lib/types";
import "diff2html/bundles/css/diff2html.min.css";
import "./DiffDialog.css";
import type { DiffDialogProps, UnpushedCommit } from "./types";
import { api } from "./api";
import { errorMessage } from "./utils";
import BaseDialog from "./BaseDialog";
import Btn from "./Btn";

type DiffTab = "diff" | "status" | "unpushed";

const diffOpts = {
  outputFormat: "line-by-line" as const,
  colorScheme: ColorSchemeType.DARK,
  drawFileList: false,
};

export default function DiffDialog({ branch, onclose }: DiffDialogProps) {
  const [uncommitted, setUncommitted] = useState("");
  const [uncommittedTruncated, setUncommittedTruncated] = useState(false);
  const [gitStatus, setGitStatus] = useState("");
  const [unpushedCommits, setUnpushedCommits] = useState<UnpushedCommit[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [activeTab, setActiveTab] = useState<DiffTab>("diff");
  const initialTabSet = useRef(false);

  useEffect(() => {
    setLoading(true);
    setError("");
    api
      .fetchWorktreeDiff({ params: { name: branch } })
      .then((res) => {
        setUncommitted(res.uncommitted);
        setUncommittedTruncated(res.uncommittedTruncated);
        setGitStatus(res.gitStatus);
        setUnpushedCommits(res.unpushedCommits);
      })
      .catch((err: unknown) => {
        setError(errorMessage(err));
      })
      .finally(() => {
        setLoading(false);
      });
  }, [branch]);

  const renderedUncommitted = useMemo(
    () => (uncommitted ? diff2html(uncommitted, diffOpts) : ""),
    [uncommitted],
  );
  const gitStatusLineCount = useMemo(
    () => (gitStatus ? gitStatus.split("\n").filter((line) => line.length > 0).length : 0),
    [gitStatus],
  );
  const hasContent = !!uncommitted || gitStatusLineCount > 0 || unpushedCommits.length > 0;

  useEffect(() => {
    if (!loading && !error && !initialTabSet.current) {
      initialTabSet.current = true;
      setActiveTab(uncommitted ? "diff" : gitStatusLineCount > 0 ? "status" : "unpushed");
    }
  }, [loading, error, uncommitted, gitStatusLineCount]);

  return (
    <BaseDialog onclose={onclose} wide maxWidth="90vw" className="diff-dialog">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-base">
          Changes &mdash; <span className="font-mono text-sm">{branch}</span>
        </h2>
      </div>

      {loading ? (
        <div className="text-sm text-muted py-8 text-center">Loading diff...</div>
      ) : error ? (
        <div className="text-sm text-danger py-8 text-center">{error}</div>
      ) : !hasContent ? (
        <div className="text-sm text-muted py-8 text-center">No changes</div>
      ) : (
        <>
          <div className="flex gap-1 mb-3">
            <button
              type="button"
              className={`tab-btn${activeTab === "diff" ? " active" : ""}`}
              disabled={!uncommitted}
              onClick={() => setActiveTab("diff")}
            >
              Current diff
            </button>
            <button
              type="button"
              className={`tab-btn${activeTab === "status" ? " active" : ""}`}
              disabled={gitStatusLineCount === 0}
              onClick={() => setActiveTab("status")}
            >
              Git status ({gitStatusLineCount})
            </button>
            <button
              type="button"
              className={`tab-btn${activeTab === "unpushed" ? " active" : ""}`}
              disabled={unpushedCommits.length === 0}
              onClick={() => setActiveTab("unpushed")}
            >
              Unpushed commits ({unpushedCommits.length})
            </button>
          </div>

          {activeTab === "diff" && uncommitted ? (
            <div className="diff-container overflow-auto max-h-[60vh] md:max-h-[70vh] rounded-md border border-edge">
              {uncommittedTruncated && (
                <div className="text-[11px] text-warning px-3 py-1">Truncated (exceeded 200KB)</div>
              )}
              <div dangerouslySetInnerHTML={{ __html: renderedUncommitted }} />
            </div>
          ) : activeTab === "status" && gitStatusLineCount > 0 ? (
            <div className="overflow-auto max-h-[60vh] md:max-h-[70vh] rounded-md border border-edge">
              <div className="px-3 py-2 text-[11px] text-muted border-b border-edge bg-surface font-mono">
                git status --short
              </div>
              <pre className="git-status-output">{gitStatus}</pre>
            </div>
          ) : activeTab === "unpushed" && unpushedCommits.length > 0 ? (
            <ul className="commit-list overflow-auto max-h-[60vh] md:max-h-[70vh] rounded-md border border-edge list-none m-0 p-0">
              {unpushedCommits.map((commit) => (
                <li
                  key={commit.hash}
                  className="flex items-baseline gap-2 px-3 py-1.5 border-b border-edge last:border-b-0"
                >
                  <code className="text-[11px] text-accent shrink-0">{commit.hash}</code>
                  <span className="text-[12px] text-primary">{commit.message}</span>
                </li>
              ))}
            </ul>
          ) : null}
        </>
      )}

      <div className="flex justify-end mt-4">
        <Btn type="button" onClick={onclose}>
          Close
        </Btn>
      </div>
    </BaseDialog>
  );
}
