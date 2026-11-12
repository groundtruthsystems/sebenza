import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import OperatorLogins from "./OperatorLogins";
import { clearOperatorToken, operatorToken } from "./operator";

type Handler = (url: string, init?: RequestInit) => { status: number; body: unknown };

function stubFetch(handler: Handler) {
  const calls: { url: string; init?: RequestInit }[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url, init });
      const { status, body } = handler(url, init);
      return new Response(JSON.stringify(body), { status });
    }),
  );
  return calls;
}

const header = (init: RequestInit | undefined, name: string) =>
  (init?.headers as Record<string, string> | undefined)?.[name];

describe("operator token", () => {
  beforeEach(() => clearOperatorToken());
  afterEach(() => {
    vi.unstubAllGlobals();
    cleanup();
  });

  it("is fetched by a same-origin POST, cached, and never put in a URL", async () => {
    const calls = stubFetch((url) =>
      url === "/api/operator/session"
        ? { status: 200, body: { token: "op_t1", expiresAt: 4_000_000_000 } }
        : { status: 404, body: {} },
    );
    expect(await operatorToken()).toBe("op_t1");
    expect(await operatorToken()).toBe("op_t1");
    expect(calls).toHaveLength(1);
    expect(calls[0].init?.method).toBe("POST");
    expect(calls[0].init?.credentials).toBe("same-origin");
    expect(calls.every((c) => !c.url.includes("op_t1"))).toBe(true);
  });

  it("is null when the daemon refuses to issue it", async () => {
    stubFetch(() => ({ status: 403, body: { code: "same_origin_required" } }));
    expect(await operatorToken()).toBeNull();
  });
});

describe("OperatorLogins", () => {
  beforeEach(() => clearOperatorToken());
  afterEach(() => {
    vi.unstubAllGlobals();
    cleanup();
  });

  // TS-51 (UI half): a pending CLI login shows its code; confirming sends the
  // operator token, never the control token.
  it("shows a pending CLI sign-in and confirms it with the operator token", async () => {
    let pending = [{ loginId: "L1", code: "A1B2C3", client: "sebenza-cli", createdAt: 1 }];
    const calls = stubFetch((url) => {
      if (url === "/api/operator/session")
        return { status: 200, body: { token: "op_t1", expiresAt: 4_000_000_000 } };
      if (url === "/api/operator/logins") return { status: 200, body: { logins: pending } };
      if (url === "/api/operator/logins/L1/confirm") {
        pending = [];
        return { status: 200, body: { ok: true } };
      }
      return { status: 404, body: {} };
    });
    render(<OperatorLogins pollMs={60_000} />);
    expect(await screen.findByText("A1B2C3")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(screen.queryByText("A1B2C3")).not.toBeInTheDocument());
    const confirm = calls.find((c) => c.url === "/api/operator/logins/L1/confirm");
    expect(confirm?.init?.method).toBe("POST");
    expect(header(confirm?.init, "authorization")).toBe("Bearer op_t1");
  });

  it("can deny, and renders nothing when no login waits", async () => {
    let pending = [{ loginId: "L2", code: "FFFFFF", client: "sebenza-cli", createdAt: 1 }];
    const calls = stubFetch((url) => {
      if (url === "/api/operator/session")
        return { status: 200, body: { token: "op_t1", expiresAt: 4_000_000_000 } };
      if (url === "/api/operator/logins") return { status: 200, body: { logins: pending } };
      if (url === "/api/operator/logins/L2/deny") {
        pending = [];
        return { status: 200, body: { ok: true } };
      }
      return { status: 404, body: {} };
    });
    const { container } = render(<OperatorLogins pollMs={60_000} />);
    await userEvent.click(await screen.findByRole("button", { name: "Deny" }));
    await waitFor(() => expect(container).toBeEmptyDOMElement());
    expect(calls.some((c) => c.url === "/api/operator/logins/L2/deny")).toBe(true);
  });

  it("refetches the token once after a 401 from a restarted daemon", async () => {
    let session = 0;
    const calls = stubFetch((url, init) => {
      if (url === "/api/operator/session") {
        session += 1;
        return { status: 200, body: { token: `op_t${session}`, expiresAt: 4_000_000_000 } };
      }
      if (url === "/api/operator/logins")
        return header(init, "authorization") === "Bearer op_t2"
          ? { status: 200, body: { logins: [] } }
          : { status: 401, body: {} };
      return { status: 404, body: {} };
    });
    render(<OperatorLogins pollMs={60_000} />);
    await waitFor(() => expect(session).toBe(2));
    const lists = calls.filter((c) => c.url === "/api/operator/logins");
    expect(lists.map((c) => header(c.init, "authorization"))).toEqual([
      "Bearer op_t1",
      "Bearer op_t2",
    ]);
  });
});
