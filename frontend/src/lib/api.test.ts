import { afterEach, describe, expect, it, vi } from "vitest";

/**
 * `apiBase` is derived from `window.location.pathname` at module load, so each
 * case sets the URL and re-imports `api.ts` fresh. Regression guard for the
 * notifications SSE + file-upload calls, which must be scoped under the active
 * project's `/<prefix>` like every other request — otherwise they fall through
 * to the hub and get `index.html` back instead of the real endpoint.
 */
async function loadApiAt(pathname: string): Promise<typeof import("./api")> {
  window.history.replaceState({}, "", pathname);
  vi.resetModules();
  return import("./api");
}

afterEach(() => {
  vi.resetModules();
  vi.unstubAllGlobals();
});

describe("project-prefixed network calls", () => {
  it("derives apiBase from the first path segment", async () => {
    expect((await loadApiAt("/myproject/")).apiBase).toBe("/myproject");
    expect((await loadApiAt("/")).apiBase).toBe("");
  });

  it("subscribeNotifications opens the SSE stream under the active prefix", async () => {
    const urls: string[] = [];
    class MockEventSource {
      constructor(url: string) {
        urls.push(url);
      }
      addEventListener(): void {}
      close(): void {}
    }
    vi.stubGlobal("EventSource", MockEventSource);

    const api = await loadApiAt("/myproject/");
    api.subscribeNotifications(
      () => {},
      () => {},
    );

    expect(urls).toEqual(["/myproject/api/notifications/stream"]);
  });

  it("uploadFiles posts under the active prefix", async () => {
    const fetchMock = vi.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ uploaded: [], dir: "x" }),
    }));
    vi.stubGlobal("fetch", fetchMock);

    const api = await loadApiAt("/myproject/");
    await api.uploadFiles("feat/x", [new File(["a"], "a.txt")]);

    expect(fetchMock).toHaveBeenCalledWith(
      "/myproject/api/worktrees/feat%2Fx/upload",
      expect.objectContaining({ method: "POST" }),
    );
  });
});

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

function urlOf(input: string | URL | Request): string {
  if (typeof input === "string") return input;
  if (input instanceof URL) return input.href;
  return input.url;
}

async function bodyOf(input: string | URL | Request, init?: RequestInit): Promise<string> {
  if (typeof init?.body === "string") return init.body;
  if (input instanceof Request) return input.text();
  return "";
}

describe("createWorktreeAgentTab", () => {
  it("posts the chosen agent to the agent-tabs route under the active prefix", async () => {
    const seen: { url: string; method: string; body: string }[] = [];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      seen.push({
        url: urlOf(input),
        method: init?.method ?? (input instanceof Request ? input.method : "GET"),
        body: await bodyOf(input, init),
      });
      // The contract declares 201 for tab creation, not 200.
      return new Response(
        JSON.stringify({
          tab: {
            tabId: "agent-codex-1",
            kind: "agent",
            label: "Codex",
            seq: 1,
            sessionId: null,
            agent: "codex",
            createdAt: "2026-01-01T00:00:00Z",
          },
        }),
        { status: 201, headers: { "content-type": "application/json" } },
      );
    });
    vi.stubGlobal("fetch", fetchMock);

    const api = await loadApiAt("/myproject/");
    const tab = await api.createWorktreeAgentTab("feat/x", "codex");

    expect(seen).toHaveLength(1);
    // Branch names contain slashes, so the path segment must be encoded.
    expect(seen[0].url).toContain("/myproject/api/worktrees/feat%2Fx/agent-tabs");
    expect(seen[0].method).toBe("POST");
    expect(JSON.parse(seen[0].body)).toEqual({ agent: "codex" });
    expect(tab.kind).toBe("agent");
    expect(tab.agent).toBe("codex");
  });
});

describe("setUpProject", () => {
  it("returns the prefix immediately when the repo is already a project", async () => {
    vi.stubGlobal("fetch", vi.fn(async () =>
      jsonResponse({
        initializing: false,
        path: "/repo/y",
        project: { prefix: "y", name: "Y", path: "/repo/y", active: false },
      }),
    ));

    const api = await loadApiAt("/y/");
    const phases: string[] = [];
    const result = await api.setUpProject("/repo/y", (phase) => phases.push(phase));

    expect(result).toEqual({ prefix: "y" });
    expect(phases).toEqual([]); // no setup needed → no phases
  });

  it("polls the setup tracker and resolves with the prefix when ready", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const url = urlOf(input);
      const method = init?.method ?? "GET";
      if (url.endsWith("/api/projects") && method === "POST") {
        return jsonResponse({ initializing: true, path: "/repo/x", project: null });
      }
      if (url.endsWith("/api/projects/init")) {
        return jsonResponse({
          inits: [{ path: "/repo/x", phase: "ready", prefix: "x", name: "X", error: null }],
        });
      }
      throw new Error(`unexpected ${method} ${url}`);
    });
    vi.stubGlobal("fetch", fetchMock);

    const api = await loadApiAt("/x/");
    const phases: string[] = [];
    const result = await api.setUpProject("/repo/x", (phase) => phases.push(phase));

    expect(result).toEqual({ prefix: "x" });
    expect(phases).toEqual(["ready"]);
  });

  it("rejects with the server error when setup fails", async () => {
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const url = urlOf(input);
      const method = init?.method ?? "GET";
      if (url.endsWith("/api/projects") && method === "POST") {
        return jsonResponse({ initializing: true, path: "/repo/z", project: null });
      }
      return jsonResponse({
        inits: [{ path: "/repo/z", phase: "failed", prefix: null, name: null, error: "not a git repo" }],
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    const api = await loadApiAt("/x/");
    await expect(api.setUpProject("/repo/z", () => {})).rejects.toThrow("not a git repo");
  });
});

/**
 * `mapWorktree` picks named fields off the snapshot rather than spreading it, so a
 * field added to the Zod contract and the Rust backend alone reaches the store as
 * `undefined` — with every component test still passing against its own stubs. These
 * cover the contract and the mapping together, because that seam is where the field
 * would silently disappear.
 */
describe("feedbackState reaches the store", () => {
  const snapshot = {
    branch: "feature",
    kind: "linked" as const,
    label: null,
    path: "/repo/feature",
    dir: "/repo/feature",
    archived: false,
    profile: null,
    agentName: null,
    agentLabel: null,
    agentTerminalStale: false,
    mux: true,
    dirty: false,
    unpushed: false,
    paneCount: 1,
    status: "awaiting_permission",
    feedbackState: "permission_request" as const,
    elapsed: "2m",
    services: [],
    prs: [],
    creation: null,
    source: "ui" as const,
    oneshot: null,
  };

  it("accepts the three feedback states and rejects anything else", async () => {
    const { ProjectWorktreeSnapshotSchema } = await import("./api-contract");

    for (const feedbackState of ["none", "permission_request", "user_question"]) {
      const parsed = ProjectWorktreeSnapshotSchema.parse({ ...snapshot, feedbackState });
      expect(parsed.feedbackState).toBe(feedbackState);
    }

    // A typo or a state this client does not know must fail loudly here rather than
    // reaching the ticker as an unhandled value.
    expect(() =>
      ProjectWorktreeSnapshotSchema.parse({ ...snapshot, feedbackState: "needs_attention" }),
    ).toThrow();
  });

  it("defaults to none when an older server omits the field", async () => {
    const { ProjectWorktreeSnapshotSchema } = await import("./api-contract");
    const { feedbackState: _omitted, ...withoutField } = snapshot;

    expect(ProjectWorktreeSnapshotSchema.parse(withoutField).feedbackState).toBe("none");
  });

  it("carries feedbackState through mapWorktree onto WorktreeInfo", async () => {
    const mod = await loadApiAt("/myproject/");
    vi.spyOn(mod.api, "fetchWorktrees").mockResolvedValue({
      project: { name: "sebenza", mainBranch: "main" },
      worktrees: [snapshot],
      notifications: [],
    } as never);

    const [worktree] = await mod.fetchWorktrees();

    expect(worktree.feedbackState).toBe("permission_request");
    // The pre-existing status mapping must keep working alongside it.
    expect(worktree.status).toBe("awaiting_permission");
    expect(worktree.agent).toBe("awaiting-permission");
  });
});

describe("inbox priority, comments and requests", () => {
  type Seen = { url: string; method: string; body: string; auth: string | null };

  function recordFetch(reply: unknown, status = 200) {
    const seen: Seen[] = [];
    const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
      const headers = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined));
      seen.push({
        url: urlOf(input),
        method: init?.method ?? (input instanceof Request ? input.method : "GET"),
        body: await bodyOf(input, init),
        auth: headers.get("authorization"),
      });
      return new Response(JSON.stringify(reply), {
        status,
        headers: { "content-type": "application/json" },
      });
    });
    vi.stubGlobal("fetch", fetchMock);
    return seen;
  }

  const draft = {
    id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
    title: "Idea",
    status: "Draft",
    createdAt: "t",
    updatedAt: "t",
    body: "",
    bodyHash: "h",
    project: null,
    priority: "P0",
    prioritySource: "operator",
    conversions: [],
    raw: null,
  };

  afterEach(() => {
    delete window.__SEBENZA_CONTROL_TOKEN__;
  });

  it("sets and clears the priority override with the control token", async () => {
    const seen = recordFetch(draft);
    window.__SEBENZA_CONTROL_TOKEN__ = "tok";
    const api = await loadApiAt("/inbox");

    const set = await api.setInboxPriority(draft.id, "P0");
    expect(set.priority).toBe("P0");
    await api.setInboxPriority(draft.id, null);

    expect(seen[0].url).toContain(`/api/inbox/${draft.id}/priority`);
    expect(seen[0].method).toBe("PATCH");
    expect(seen[0].auth).toBe("Bearer tok");
    expect(JSON.parse(seen[0].body)).toEqual({ priority: "P0" });
    // `null` must be sent explicitly: an absent key is refused, not a clear.
    expect(JSON.parse(seen[1].body)).toEqual({ priority: null });
  });

  it("reads grouped comments and posts an operator comment", async () => {
    const groups = {
      overall: [],
      worktrees: [{ project: "/code/acme-demo", branch: "feat-x", comments: [] }],
    };
    const seen = recordFetch(groups);
    window.__SEBENZA_CONTROL_TOKEN__ = "tok";
    const api = await loadApiAt("/inbox");

    const got = await api.fetchInboxComments(draft.id);
    expect(got.worktrees[0].branch).toBe("feat-x");
    expect(seen[0].url).toContain(`/api/inbox/${draft.id}/comments`);
    expect(seen[0].method).toBe("GET");

    await api.postInboxComment(draft.id, "looks good", {
      project: "/code/acme-demo",
      branch: "feat-x",
    });
    expect(seen[1].method).toBe("POST");
    expect(seen[1].auth).toBe("Bearer tok");
    expect(JSON.parse(seen[1].body)).toEqual({
      body: "looks good",
      worktree: { project: "/code/acme-demo", branch: "feat-x" },
    });

    await api.postInboxComment(draft.id, "overall note");
    expect(JSON.parse(seen[2].body)).toEqual({ body: "overall note" });
  });

  it("lists an item's requests", async () => {
    const seen = recordFetch({ requests: [] });
    const api = await loadApiAt("/inbox");
    const got = await api.fetchInboxRequests(draft.id);
    expect(got.requests).toEqual([]);
    expect(seen[0].url).toContain(`/api/inbox/${draft.id}/requests`);
  });

  it("surfaces a rate-limit refusal as the server's message", async () => {
    recordFetch({ error: "rate limit exceeded; try again shortly" }, 429);
    window.__SEBENZA_CONTROL_TOKEN__ = "tok";
    const api = await loadApiAt("/inbox");
    await expect(api.postInboxComment(draft.id, "again")).rejects.toThrow(/rate limit/);
  });
});

describe("inbox priority schema", () => {
  it("defaults priority for a server that predates it", async () => {
    const { InboxDraftSummarySchema } = await import("./api-contract");
    const parsed = InboxDraftSummarySchema.parse({
      id: "x",
      title: "t",
      status: "Draft",
      updatedAt: "t",
      project: null,
      isRaw: false,
    });
    expect(parsed.priority).toBe("P2");
    expect(parsed.prioritySource).toBe("agent");
  });
});
