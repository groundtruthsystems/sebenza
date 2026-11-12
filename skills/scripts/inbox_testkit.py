"""Test fixtures for inbox.py: a linked git worktree, a fake Sebenza daemon and a fake agentctl.

Not a test module itself (no ``test_`` prefix), so unittest discovery skips it.
Everything is synthetic: the control token is a fixed sentinel and nothing
touches the real checkout, the real daemon or ``~/.ai/sebenza``.
"""

import contextlib
import io
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

import inbox  # noqa: E402

TOKEN = "TEST-TOKEN-0000"
DRAFT = "01TESTDRAFT0000000000000000"
BRANCH = "feat-graph"
TRACK = "demo_track_20260101"

FAKE_AGENTCTL = r'''#!{python}
"""Fake sebenza-agentctl: logs how it was called, then behaves as configured."""
import json, os, sys, datetime
here = os.path.dirname(os.path.abspath(__file__))
cfg = json.load(open(os.path.join(here, "fake-agentctl.json")))
args = sys.argv[1:]
stdin = sys.stdin.read() if "-" in args else ""
with open(os.path.join(here, "agentctl.log"), "a") as log:
    log.write(json.dumps({{"argv": args, "stdin": stdin,
        "env_worktree_path": os.environ.get("SEBENZA_WORKTREE_PATH"),
        "env_has_token": "SEBENZA_CONTROL_TOKEN" in os.environ,
        "env_values": list(os.environ.values())}}) + "\n")
if cfg.get("reject_flags") and ("--track" in args or "--task" in args):
    sys.stderr.write(cfg.get("reject_message") or "error: unexpected argument '--track' found\n")
    sys.exit(2)
if cfg.get("stderr"):
    sys.stderr.write(cfg["stderr"])
if args and args[0] == "review":
    sys.stdout.write(json.dumps(cfg.get("review_stdout") or {{}}))
    sys.exit(cfg.get("review_exit", 0))
code = cfg.get("exit", 0)
if code == 0 and cfg.get("append_request", True) and args and args[0] == "request":
    title = args[args.index("--title") + 1] if "--title" in args else stdin.splitlines()[0]
    state_path = cfg["state"]
    state = json.load(open(state_path))
    n = len(state["requests"]) + 1
    state["requests"].append({{"requestId": "REQ%04d" % n,
        "worktree": {{"project": "/repo/main", "branch": cfg["branch"]}},
        "title": title, "body": stdin, "status": "open", "flagged": False,
        "confirmedText": None, "warnings": [], "attempts": 0,
        "openedAt": "2026-11-05T12:%02d:00Z" % n,
        "trackId": args[args.index("--track") + 1] if "--track" in args else None,
        "taskId": args[args.index("--task") + 1] if "--task" in args else None}})
    json.dump(state, open(state_path, "w"))
sys.exit(code)
'''


def git(cwd, *args):
    return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True,
                          text=True).stdout.strip()


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class FakeDaemon:
    """Serves GET /api/inbox/{id}/requests and /comments from a JSON state file."""

    def __init__(self, state_path, port=0):
        self.state_path = state_path
        self.hits = []
        self.mode = "ok"  # ok | redirect | garbage
        daemon = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *a):  # silence
                pass

            def do_GET(self):  # noqa: N802
                daemon.hits.append(self.path)
                if daemon.mode == "redirect":
                    self.send_response(302)
                    self.send_header("Location", "http://example.com/")
                    self.end_headers()
                    return
                with open(daemon.state_path) as fh:
                    state = json.load(fh)
                if self.path == f"/api/inbox/{DRAFT}/requests":
                    body = {"requests": state["requests"]}
                elif self.path == f"/api/inbox/{DRAFT}/comments":
                    body = state["comments"]
                else:
                    self.send_response(404)
                    self.end_headers()
                    return
                raw = b"not json" if daemon.mode == "garbage" else json.dumps(body).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(raw)

        self.server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
        self.port = self.server.server_address[1]
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()


class Workspace:
    """A main repo plus a linked worktree converted from inbox item DRAFT."""

    def __init__(self, origin=True, daemon=True, agentctl=True, control_url=None):
        self.tmp = tempfile.mkdtemp(prefix="sebenza-inbox-test-")
        self.main = os.path.join(self.tmp, "main")
        os.makedirs(self.main)
        git(self.main, "init", "-q", "-b", "main")
        git(self.main, "-c", "user.email=t@example.invalid", "-c", "user.name=t",
            "commit", "-q", "--allow-empty", "-m", "init")
        self.wt = os.path.join(self.tmp, "worktrees", "wt")
        git(self.main, "worktree", "add", "-q", "-b", BRANCH, self.wt)
        self.wt = os.path.realpath(self.wt)
        # The same worktree spelt the way Sebenza's env spells it: through `/../`.
        self.wt_dotted = os.path.join(self.main, "..", "worktrees", "wt")
        self.ws = os.path.join(self.wt, ".ai", "sebenza")
        os.makedirs(os.path.join(self.ws, "tracks", TRACK))
        self.gitdir = git(self.wt, "rev-parse", "--absolute-git-dir")
        self.sdir = os.path.join(self.gitdir, ".ai", "sebenza")
        os.makedirs(self.sdir, exist_ok=True)
        self.state_path = os.path.join(self.tmp, "daemon-state.json")
        self.set_state(requests=[], comments={"overall": [], "worktrees": []})
        self.daemon = FakeDaemon(self.state_path) if daemon else None
        port = self.daemon.port if daemon else free_port()
        self.port = port
        if origin:
            self.write_json(os.path.join(self.ws, "inbox-origin.json"), {"draftId": DRAFT})
        url = control_url or f"http://127.0.0.1:{port}/api/runtime/events"
        self.write_control_env({"SEBENZA_CONTROL_URL": url, "SEBENZA_CONTROL_TOKEN": TOKEN,
                                "SEBENZA_BRANCH": BRANCH, "SEBENZA_WORKTREE_ID": "wt-0001"})
        if agentctl:
            path = os.path.join(self.sdir, "sebenza-agentctl")
            with open(path, "w") as fh:
                fh.write(FAKE_AGENTCTL.format(python=sys.executable))
            os.chmod(path, 0o755)
            self.configure_agentctl()
        self.write_tracks()

    # -- setup helpers -------------------------------------------------------

    def write_json(self, path, data):
        with open(path, "w") as fh:
            json.dump(data, fh, indent=2)

    def read_json(self, path):
        with open(path) as fh:
            return json.load(fh)

    def write_control_env(self, values):
        with open(os.path.join(self.sdir, "control.env"), "w") as fh:
            for k, v in values.items():
                fh.write(f"{k}={v}\n")

    def configure_agentctl(self, exit=0, stderr="", append_request=True, reject_flags=False,
                           reject_message=None, review_stdout=None, review_exit=0):
        self.write_json(os.path.join(self.sdir, "fake-agentctl.json"),
                        {"exit": exit, "stderr": stderr, "append_request": append_request,
                         "reject_flags": reject_flags, "reject_message": reject_message,
                         "review_stdout": review_stdout, "review_exit": review_exit,
                         "state": self.state_path, "branch": BRANCH})

    def set_state(self, requests=None, comments=None):
        state = self.read_json(self.state_path) if os.path.exists(self.state_path) else {}
        if requests is not None:
            state["requests"] = requests
        if comments is not None:
            state["comments"] = comments
        self.write_json(self.state_path, state)

    def write_tracks(self, task_status="doing", task_extra=None, track_extra=None,
                     phase_status="doing", track_status="doing", with_plan=True):
        task = {"id": "t1", "name": "Task one", "status": task_status}
        task.update(task_extra or {})
        plan = {"$schema": "sebenza-plan-v1", "track_id": TRACK, "phases": [
            {"id": "phase-1", "name": "P1", "status": phase_status, "blocked_reason": None,
             "tasks": [task, {"id": "t2", "name": "Task two", "status": "backlog"}]}]}
        track = {"track_id": TRACK, "type": "feature", "description": "demo",
                 "status": track_status, "created_at": "2026-01-01T00:00:00Z",
                 "updated_at": "2026-01-01T00:00:00Z",
                 "phases_summary": [{"id": "phase-1", "name": "P1", "status": phase_status}],
                 "progress": {"total_tasks": 2, "completed_tasks": 0, "percentage": 0},
                 "unknown_future_key": {"keep": True}}
        if with_plan:
            track["plan_path"] = f"./tracks/{TRACK}/plan.json"
            self.write_json(self.plan_path, plan)
        track.update(track_extra or {})
        self.write_json(self.tracks_path, {
            "$schema": "sebenza-tracks-v1", "version": "1.0",
            "project": {"name": "demo", "path": self.wt, "registered_at": "2026-01-01T00:00:00Z"},
            "tracks": [track]})

    @property
    def plan_path(self):
        return os.path.join(self.ws, "tracks", TRACK, "plan.json")

    @property
    def tracks_path(self):
        return os.path.join(self.ws, "tracks.json")

    def task(self, task_id="t1"):
        plan = self.read_json(self.plan_path)
        return next(t for p in plan["phases"] for t in p["tasks"] if t["id"] == task_id)

    def agentctl_calls(self):
        path = os.path.join(self.sdir, "agentctl.log")
        if not os.path.exists(path):
            return []
        with open(path) as fh:
            return [json.loads(line) for line in fh if line.strip()]

    def close(self):
        if self.daemon:
            self.daemon.close()
        shutil.rmtree(self.tmp, ignore_errors=True)


def run(args, cwd, stdin=""):
    """Run inbox.main in-process (so the trace coverage gate sees it).

    Returns (exit_code, parsed_stdout_json_or_None, raw_stdout, stderr).
    """
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = inbox.main(list(args), stdin=io.StringIO(stdin), cwd=cwd)
    raw = out.getvalue()
    try:
        parsed = json.loads(raw) if raw.strip() else None
    except ValueError:
        parsed = None
    return code, parsed, raw, err.getvalue()


def request(i, title, status="open", branch=BRANCH, opened="2026-11-05T10:00:00Z", **extra):
    r = {"requestId": f"R{i}", "worktree": {"project": "/repo/main", "branch": branch},
         "title": title, "body": "b", "status": status, "flagged": False,
         "confirmedText": None, "warnings": [], "attempts": 0, "openedAt": opened}
    r.update(extra)
    return r


def comment(i, author, body, ts="2026-11-05T11:00:00Z", kind="note"):
    return {"eventId": f"E{i}", "ts": ts, "author": author, "caller": None, "kind": kind,
            "body": body, "title": None, "requestId": None, "parentEventId": None,
            "warnings": [], "redacted": False}
