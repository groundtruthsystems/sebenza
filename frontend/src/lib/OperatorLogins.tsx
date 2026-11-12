import { useEffect, useState } from "react";
import { decideLogin, fetchPendingLogins, type PendingLogin } from "./operator";

const POLL_MS = 3000;

/** `sebenza-cli login` requests waiting for the operator (TS-51). The code
 *  shown here must match the one the CLI printed; confirming hands that CLI
 *  an operator token, so a code you did not start should be denied. */
export default function OperatorLogins({ pollMs = POLL_MS }: { pollMs?: number }) {
  const [logins, setLogins] = useState<PendingLogin[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    async function load(): Promise<void> {
      try {
        const next = await fetchPendingLogins();
        if (alive) setLogins(next);
      } catch {
        if (alive) setLogins([]);
      }
    }
    void load();
    const timer = setInterval(() => void load(), pollMs);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [pollMs]);

  async function decide(id: string, decision: "confirm" | "deny"): Promise<void> {
    setBusy(id);
    setError("");
    try {
      await decideLogin(id, decision);
      setLogins((all) => all.filter((l) => l.loginId !== id));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  if (logins.length === 0) return null;
  return (
    <div
      role="alert"
      aria-label="Operator sign-in requests"
      className="flex flex-col gap-1 px-4 py-2 text-[13px] bg-surface border-b border-edge text-primary"
    >
      {logins.map((l) => (
        <div key={l.loginId} className="flex flex-wrap items-center gap-3">
          <span className="flex-1 min-w-0">
            <span className="text-amber-400 font-medium">{l.client}</span> asks to sign in as the
            operator with code <code className="text-primary font-semibold">{l.code}</code>. Confirm
            only if you started it and the code matches.
          </span>
          <button
            type="button"
            className="px-2 py-0.5 rounded border border-edge hover:border-accent"
            disabled={busy === l.loginId}
            onClick={() => void decide(l.loginId, "confirm")}
          >
            Confirm
          </button>
          <button
            type="button"
            className="px-2 py-0.5 rounded border border-edge text-muted hover:text-primary"
            disabled={busy === l.loginId}
            onClick={() => void decide(l.loginId, "deny")}
          >
            Deny
          </button>
        </div>
      ))}
      {error && <span className="text-danger">{error}</span>}
    </div>
  );
}
