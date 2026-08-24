#!/usr/bin/env python3
import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path


CONTROL_ENV_PATH = Path(__file__).resolve().with_name("control.env")
CONTROL_REQUEST_TIMEOUT_SECONDS = 2


def read_control_env():
    env = {}
    try:
        content = CONTROL_ENV_PATH.read_text()
    except OSError as error:
        raise RuntimeError(f"failed to read control.env: {error}") from error

    for raw_line in content.splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        if len(value) >= 2 and value.startswith("'") and value.endswith("'"):
            value = value[1:-1].replace("'\\''", "'")
        env[key] = value

    return env


def build_parser():
    parser = argparse.ArgumentParser(prog="sebenza-agentctl")
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("agent-stopped")

    status_changed = subparsers.add_parser("status-changed")
    status_changed.add_argument("--lifecycle", choices=["starting", "running", "idle", "stopped"], required=True)
    status_changed.add_argument("--best-effort", action="store_true")

    pr_opened = subparsers.add_parser("pr-opened")
    pr_opened.add_argument("--url")

    runtime_error = subparsers.add_parser("runtime-error")
    runtime_error.add_argument("--message", required=True)

    subparsers.add_parser("claude-user-prompt-submit")
    subparsers.add_parser("claude-post-tool-use")
    subparsers.add_parser("codex-session-start")
    subparsers.add_parser("codex-user-prompt-submit")
    subparsers.add_parser("codex-permission-request")
    subparsers.add_parser("codex-post-tool-use")
    subparsers.add_parser("codex-stop")
    # opencode: driven by the generated JS plugin (.opencode/plugins/sebenza.js), which
    # pipes a small derived payload on stdin. Mapped onto the same three primitives the
    # claude/codex subcommands use - status-changed, agent-stopped, PR detection - so the
    # decision logic lives here in one language rather than being reimplemented in JS.
    subparsers.add_parser("opencode-session-created")
    subparsers.add_parser("opencode-tool-before")
    subparsers.add_parser("opencode-tool-after")
    subparsers.add_parser("opencode-permission-ask")
    subparsers.add_parser("opencode-permission-asked")
    subparsers.add_parser("opencode-permission-replied")
    subparsers.add_parser("opencode-stop")
    # grok: Claude-Code-shaped hook JSON, but a camelCase stdin envelope (sessionId,
    # toolName, toolInput) and `toolResult` where Claude has `tool_response`. These
    # subcommands normalise before reusing the shared primitives, so the claude-* handlers
    # never see a grok payload.
    subparsers.add_parser("grok-session-start")
    subparsers.add_parser("grok-user-prompt-submit")
    subparsers.add_parser("grok-post-tool-use")
    subparsers.add_parser("grok-permission-prompt")
    subparsers.add_parser("grok-stop")

    return parser


def build_payload(command, args, control_env):
    payload = {
        "worktreeId": control_env["SEBENZA_WORKTREE_ID"],
        "branch": control_env["SEBENZA_BRANCH"],
    }

    if command == "agent-stopped":
        payload["type"] = "agent_stopped"
        return payload
    if command == "status-changed":
        payload["type"] = "agent_status_changed"
        payload["lifecycle"] = args.lifecycle
        return payload
    if command == "pr-opened":
        payload["type"] = "pr_opened"
        if args.url:
            payload["url"] = args.url
        return payload
    if command == "runtime-error":
        payload["type"] = "runtime_error"
        payload["message"] = args.message
        return payload
    if command == "conversation-started":
        # sessionId is attached by the caller, which reads it from the hook payload.
        payload["type"] = "conversation_started"
        return payload
    raise RuntimeError(f"unsupported command: {command}")


def read_hook_payload():
    raw = sys.stdin.read()
    if not raw.strip():
        return {}

    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError:
        return {}

    return parsed if isinstance(parsed, dict) else {}


def iter_string_values(value):
    if isinstance(value, str):
        yield value
        return
    if isinstance(value, dict):
        for child in value.values():
            yield from iter_string_values(child)
        return
    if isinstance(value, list):
        for child in value:
            yield from iter_string_values(child)


def find_pr_url(value):
    for text in iter_string_values(value):
        match = re.search(r"https://github\.com/[^\s\"]+/pull/\d+", text)
        if match:
            return match.group(0)
    return None


def maybe_send_pr_opened(hook_payload, control_env):
    tool_name = hook_payload.get("tool_name")
    tool_input = hook_payload.get("tool_input")
    if not isinstance(tool_input, dict) or tool_name != "Bash":
        return True

    command = tool_input.get("command")
    if not isinstance(command, str) or "gh pr create" not in command:
        return True

    pr_args = argparse.Namespace(url=find_pr_url(hook_payload.get("tool_response")))
    return send_payload(build_payload("pr-opened", pr_args, control_env), control_env)


# grok's shell tool. `--help` documents `run_terminal_command`; the headless page also
# spells it `run_terminal_cmd`, so accept both rather than betting on one.
GROK_SHELL_TOOLS = ("run_terminal_command", "run_terminal_cmd", "Bash")

# Reasons that mean a turn actually ended. `end_turn` is a completed turn; the rest are the
# StopCancelled reasons, which must still report stopped - that is why StopCancelled is
# hooked at all. Any other reason is the extra observe-only Stop grok fires at session end,
# which would otherwise report a second spurious stop after the user already quit.
GROK_TURN_END_REASONS = (
    "end_turn",
    "user_interrupt",
    "permission_rejected",
    "permission_cancelled",
    "max_turns",
    "no_progress",
    "unknown",
)


def normalize_grok_hook_payload(hook_payload):
    """Map grok's camelCase hook envelope onto the snake_case shape the shared helpers read.

    Without this, maybe_send_pr_opened silently never fires for grok: it looks for
    `tool_name`/`tool_input`/`tool_response`, and grok sends `toolName`/`toolInput`/
    `toolResult`. The tool name is also grok's own, so it is mapped to `Bash` - the name the
    shared PR check matches on.
    """
    if not isinstance(hook_payload, dict):
        return {}

    tool_name = hook_payload.get("toolName")
    normalized = dict(hook_payload)
    if tool_name in GROK_SHELL_TOOLS:
        normalized["tool_name"] = "Bash"
    elif isinstance(tool_name, str):
        normalized["tool_name"] = tool_name
    if isinstance(hook_payload.get("toolInput"), dict):
        normalized["tool_input"] = hook_payload["toolInput"]
    if "toolResult" in hook_payload:
        normalized["tool_response"] = hook_payload["toolResult"]
    return normalized


def grok_is_subagent(hook_payload):
    """True when this event came from a subagent's own session, not the main one.

    grok sets `subagentType` only inside a subagent. A background subagent outlives the
    parent turn, so without this filter its events would hold the worktree at "running"
    after the main agent already went idle.
    """
    return bool(isinstance(hook_payload, dict) and hook_payload.get("subagentType"))


def send_payload(payload, control_env):
    request = urllib.request.Request(
        control_env["SEBENZA_CONTROL_URL"],
        data=json.dumps(payload).encode(),
        headers={
            "Authorization": f"Bearer {control_env['SEBENZA_CONTROL_TOKEN']}",
            "Content-Type": "application/json",
        },
        method="POST",
    )

    try:
        with urllib.request.urlopen(request, timeout=CONTROL_REQUEST_TIMEOUT_SECONDS) as response:
            if response.status < 200 or response.status >= 300:
                print(f"control endpoint returned HTTP {response.status}", file=sys.stderr)
                return False
    except urllib.error.HTTPError as error:
        print(f"control endpoint returned HTTP {error.code}", file=sys.stderr)
        return False
    except Exception as error:
        print(f"failed to send runtime event: {error}", file=sys.stderr)
        return False

    return True


def main():
    parsed = build_parser().parse_args()

    try:
        control_env = read_control_env()
    except RuntimeError as error:
        print(str(error), file=sys.stderr)
        return 1

    required_keys = [
        "SEBENZA_CONTROL_URL",
        "SEBENZA_CONTROL_TOKEN",
        "SEBENZA_WORKTREE_ID",
        "SEBENZA_BRANCH",
    ]
    missing = [key for key in required_keys if not control_env.get(key)]
    if missing:
        print(f"missing control env keys: {', '.join(missing)}", file=sys.stderr)
        return 1

    if parsed.command == "codex-session-start":
        send_payload(build_payload("status-changed", argparse.Namespace(lifecycle="idle"), control_env), control_env)
        return 0

    if parsed.command == "codex-user-prompt-submit":
        send_payload(build_payload("status-changed", argparse.Namespace(lifecycle="running"), control_env), control_env)
        return 0

    if parsed.command == "claude-user-prompt-submit":
        if not send_payload(build_payload("status-changed", argparse.Namespace(lifecycle="running"), control_env), control_env):
            return 1
        return 0

    if parsed.command == "codex-permission-request":
        send_payload(build_payload("status-changed", argparse.Namespace(lifecycle="idle"), control_env), control_env)
        return 0

    if parsed.command == "codex-post-tool-use":
        hook_payload = read_hook_payload()
        maybe_send_pr_opened(hook_payload, control_env)
        return 0

    if parsed.command == "claude-post-tool-use":
        hook_payload = read_hook_payload()
        return 0 if maybe_send_pr_opened(hook_payload, control_env) else 1

    if parsed.command == "opencode-session-created":
        # Report the session id: this is the ONLY route by which Sebenza learns it, since
        # opencode's store is SQLite behind an internal schema and `session list` has no
        # directory column. Then mark the agent running.
        hook_payload = read_hook_payload()
        session_id = (hook_payload or {}).get("sessionID")
        if session_id:
            payload = build_payload("conversation-started", parsed, control_env)
            payload["sessionId"] = session_id
            send_payload(payload, control_env)
        send_payload(build_payload("status-changed", argparse.Namespace(lifecycle="running"), control_env), control_env)
        return 0

    if parsed.command == "opencode-tool-before":
        send_payload(build_payload("status-changed", argparse.Namespace(lifecycle="running"), control_env), control_env)
        return 0

    if parsed.command == "opencode-permission-asked":
        # Blocked on a human decision in opencode's own TUI. A DISTINCT lifecycle rather
        # than plain idle, so the dashboard can say WHY this worktree wants attention:
        # approve something already proposed, versus send the next prompt. With many
        # parallel worktrees that difference is the whole value of the signal.
        #
        # Sebenza cannot answer the prompt - opencode's permission.ask hook does not fire
        # (verified on 1.18.9) - so this is observational only.
        send_payload(build_payload("status-changed", argparse.Namespace(lifecycle="awaiting_permission"), control_env), control_env)
        return 0

    if parsed.command == "opencode-permission-replied":
        send_payload(build_payload("status-changed", argparse.Namespace(lifecycle="running"), control_env), control_env)
        return 0

    if parsed.command == "opencode-permission-ask":
        # opencode is waiting on a permission decision, so it is idle from Sebenza's point
        # of view. Sebenza cannot yet answer it - see the phase-3 spike.
        send_payload(build_payload("status-changed", argparse.Namespace(lifecycle="idle"), control_env), control_env)
        return 0

    if parsed.command == "opencode-tool-after":
        hook_payload = read_hook_payload()
        maybe_send_pr_opened(hook_payload, control_env)
        return 0

    if parsed.command == "opencode-stop":
        send_payload(build_payload("agent-stopped", parsed, control_env), control_env)
        return 0

    if parsed.command.startswith("grok-"):
        hook_payload = read_hook_payload()
        # A subagent's events describe the child, not this worktree.
        if grok_is_subagent(hook_payload):
            return 0

        if parsed.command == "grok-session-start":
            # grok pins its session id via `-s` at launch, so this is a cross-check rather
            # than the only route (contrast opencode). GROK_SESSION_ID is injected into
            # every hook process, so prefer it and fall back to the stdin envelope.
            session_id = os.environ.get("GROK_SESSION_ID") or hook_payload.get("sessionId")
            if session_id:
                payload = build_payload("conversation-started", parsed, control_env)
                payload["sessionId"] = session_id
                send_payload(payload, control_env)
            send_payload(build_payload("status-changed", argparse.Namespace(lifecycle="idle"), control_env), control_env)
            return 0

        if parsed.command == "grok-user-prompt-submit":
            send_payload(build_payload("status-changed", argparse.Namespace(lifecycle="running"), control_env), control_env)
            return 0

        if parsed.command == "grok-permission-prompt":
            # grok is blocked on a human decision in its own TUI. A distinct lifecycle from
            # plain idle so the dashboard can say WHY the worktree wants attention: approve
            # something already proposed, versus send the next prompt.
            send_payload(build_payload("status-changed", argparse.Namespace(lifecycle="awaiting_permission"), control_env), control_env)
            return 0

        if parsed.command == "grok-post-tool-use":
            maybe_send_pr_opened(normalize_grok_hook_payload(hook_payload), control_env)
            return 0

        if parsed.command == "grok-stop":
            # An extra observe-only Stop fires at session end, which would report a second
            # spurious stop after the user has already quit. Genuine turn ends carry
            # reason == "end_turn". StopCancelled has its own reasons (user_interrupt,
            # permission_rejected, max_turns, ...) and must NOT be filtered out - that is
            # the whole point of hooking it.
            reason = hook_payload.get("reason")
            if reason is not None and reason not in GROK_TURN_END_REASONS:
                return 0
            send_payload(build_payload("agent-stopped", parsed, control_env), control_env)
            return 0

    if parsed.command == "codex-stop":
        send_payload(build_payload("agent-stopped", parsed, control_env), control_env)
        print(json.dumps({}))
        return 0

    payload = build_payload(parsed.command, parsed, control_env)
    if not send_payload(payload, control_env):
        return 0 if getattr(parsed, "best_effort", False) else 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
