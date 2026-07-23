import { useEffect, useState } from "react";
import { marked } from "marked";
import { fetchConductorFile } from "./api";
import { errorMessage } from "./utils";

/** Renders a conductor markdown file (spec.md / design.md) for a worktree.
 *  Content is the user's own local worktree file (trusted), so the parsed HTML
 *  is injected directly. */
export default function ConductorMarkdown({ branch, path }: { branch: string; path: string }) {
  const [html, setHtml] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError("");
    fetchConductorFile(branch, path)
      .then((res) => {
        if (cancelled) return;
        setHtml(marked.parse(res.content, { async: false }) as string);
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

  if (loading) return <div className="text-sm text-muted py-8 text-center">Loading…</div>;
  if (error) return <div className="text-sm text-danger py-8 text-center">{error}</div>;
  return <div className="md-body" dangerouslySetInnerHTML={{ __html: html }} />;
}
