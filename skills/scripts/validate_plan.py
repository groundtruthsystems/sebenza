#!/usr/bin/env python3
"""Check the task-dependency graph of Sebenza plans.

    python3 validate_plan.py <plan.json> [<plan.json> ...]
    python3 validate_plan.py --workspace .ai/sebenza

JSON Schema checks the shape of each ``depends_on`` reference; this script
checks what a schema cannot: that references point somewhere real and that
the graph has no cycles. It prints one JSON object:

    {"ok": bool, "errors": [...], "warnings": [...], "skipped": [...]}

Errors (exit 1): a malformed reference, a task depending on itself, a cycle,
or a same-plan reference to a task that does not exist (dangling).
Warnings (exit 0): a cross-track reference this workspace cannot resolve, or a
``current_phase_id`` in tracks.json that names no phase of the plan.
Skipped: ``@branch:`` references to sibling worktrees, which only the daemon
can see. Exit 2: a file is missing or is not JSON.

Reference grammar (agreed with sebenza-improvements, see
shared/inbox-contract.md): ``["@" branch ":"] [track_id "/"] task_id`` with
ids ``[A-Za-z0-9_.-]{1,64}``, never ``.`` or ``..``. The first ``:`` ends the
branch; track and task split at the last ``/``.

Stdlib only (Python >= 3.9).
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys

OK, INVALID, USAGE = 0, 1, 2
PLAN_ID = re.compile(r"^[A-Za-z0-9_.-]{1,64}$")


def is_plan_id(s):
    return bool(PLAN_ID.match(s)) and s not in (".", "..")


def parse_ref(raw):
    """(branch, track_id, task_id) or None when malformed; mirrors the server's DepRef::parse."""
    if not isinstance(raw, str):
        return None
    raw = raw.strip()
    branch = None
    if raw.startswith("@"):
        branch, sep, raw = raw[1:].partition(":")
        if not sep or not branch or re.search(r"\s", branch):
            return None
    track, sep, task = raw.rpartition("/")
    track = track if sep else None
    if not is_plan_id(task) or (track is not None and not is_plan_id(track)):
        return None
    return branch, track, task


class UsageError(Exception):
    pass


def load(path):
    try:
        with open(path, encoding="utf-8") as fh:
            return json.load(fh)
    except (OSError, ValueError) as exc:
        raise UsageError(f"{path}: {type(exc).__name__}: {exc}") from None


def task_ids(plan):
    return {t.get("id") for p in plan.get("phases", []) for t in p.get("tasks", [])}


def plans_beside(plan_path):
    """Other plans in the same workspace, keyed by track_id, for cross-track lookups."""
    tracks_dir = os.path.dirname(os.path.dirname(os.path.abspath(plan_path)))
    found = {}
    if os.path.basename(tracks_dir) != "tracks" or not os.path.isdir(tracks_dir):
        return found
    for name in sorted(os.listdir(tracks_dir)):
        path = os.path.join(tracks_dir, name, "plan.json")
        if os.path.isfile(path) and not os.path.islink(path):
            try:
                found[name] = load(path)
            except UsageError:
                continue
    return found


def check(plans, extra_warnings=()):
    """plans: {track_id: plan}. Returns the result object."""
    errors, warnings, skipped = [], list(extra_warnings), []
    edges = {}
    ids = {tid: task_ids(p) for tid, p in plans.items()}
    for tid, plan in plans.items():
        for phase in plan.get("phases", []):
            for task in phase.get("tasks", []):
                node = f"{tid}/{task.get('id')}"
                for raw in task.get("depends_on") or []:
                    ref = parse_ref(raw)
                    entry = {"item": node, "ref": raw}
                    if ref is None:
                        errors.append(dict(entry, kind="malformed"))
                        continue
                    branch, track, target = ref
                    if branch is not None:
                        skipped.append(entry)
                        continue
                    track = track or tid
                    if track not in ids:
                        warnings.append(dict(entry, kind="unresolved"))
                        continue
                    if target not in ids[track]:
                        if track == tid:
                            errors.append(dict(entry, kind="dangling"))
                        else:
                            warnings.append(dict(entry, kind="unresolved"))
                        continue
                    if f"{track}/{target}" == node:
                        errors.append(dict(entry, kind="self"))
                        continue
                    edges.setdefault(node, []).append(f"{track}/{target}")
    errors.extend(find_cycles(edges))
    return {"ok": not errors, "errors": errors, "warnings": warnings, "skipped": skipped}


def find_cycles(edges):
    """One error per cycle found by a depth-first search (iterative)."""
    WHITE, GREY, BLACK = 0, 1, 2
    colour, out = {}, []
    for start in sorted(edges):
        if colour.get(start, WHITE) != WHITE:
            continue
        stack = [(start, iter(edges.get(start, ())))]
        path = [start]
        colour[start] = GREY
        while stack:
            node, it = stack[-1]
            nxt = next(it, None)
            if nxt is None:
                colour[node] = BLACK
                stack.pop()
                path.pop()
            elif colour.get(nxt, WHITE) == GREY:
                cycle = path[path.index(nxt):] + [nxt]
                out.append({"item": nxt, "ref": " -> ".join(cycle), "kind": "cycle"})
            elif colour.get(nxt, WHITE) == WHITE:
                colour[nxt] = GREY
                path.append(nxt)
                stack.append((nxt, iter(edges.get(nxt, ()))))
    return out


def validate_files(paths):
    plans = {}
    targets = set()
    for path in paths:
        plan = load(path)
        tid = plan.get("track_id") or os.path.basename(os.path.dirname(os.path.abspath(path)))
        targets.add(tid)
        for other, p in plans_beside(path).items():
            plans.setdefault(other, p)
        plans[tid] = plan
    result = check(plans)
    # Only report on the plans that were asked about.
    for key in ("errors", "warnings", "skipped"):
        result[key] = [e for e in result[key] if e["item"].split("/", 1)[0] in targets]
    result["ok"] = not result["errors"]
    return result


def validate_workspace(ws):
    tracks_path = os.path.join(ws, "tracks.json")
    if not os.path.isfile(tracks_path):
        return {"ok": True, "errors": [], "warnings": [], "skipped": []}
    tracks = load(tracks_path)
    plans, warnings = {}, []
    for track in tracks.get("tracks", []):
        rel = track.get("plan_path")
        if not rel:
            continue
        path = os.path.normpath(os.path.join(ws, rel) if not rel.startswith(".ai/sebenza/")
                                else os.path.join(os.path.dirname(os.path.dirname(ws)), rel))
        if not os.path.isfile(path):
            continue
        plan = load(path)
        tid = track.get("track_id")
        plans[tid] = plan
        cur = track.get("current_phase_id")
        if cur is not None and cur not in {p.get("id") for p in plan.get("phases", [])}:
            warnings.append({"item": tid, "ref": cur, "kind": "current_phase_id"})
    return check(plans, warnings)


def main(argv=None):
    p = argparse.ArgumentParser(prog="validate_plan.py", description=__doc__.split("\n\n")[0])
    p.add_argument("plans", nargs="*", help="plan.json files")
    p.add_argument("--workspace", help="a .ai/sebenza directory: check every plan tracks.json lists")
    args = p.parse_args(argv)
    try:
        if args.workspace:
            result = validate_workspace(args.workspace)
        elif args.plans:
            result = validate_files(args.plans)
        else:
            raise UsageError("give plan.json paths or --workspace")
    except UsageError as exc:
        print(json.dumps({"ok": False, "error": str(exc)}))
        return USAGE
    print(json.dumps(result, ensure_ascii=False))
    return OK if result["ok"] else INVALID


if __name__ == "__main__":
    sys.exit(main())
