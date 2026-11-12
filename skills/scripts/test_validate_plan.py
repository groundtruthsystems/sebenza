"""Tests for the graph fields in the plan/tracks schemas and for validate_plan.py.

Schema checks need ``jsonschema`` (a dev-time tool, never a runtime dependency of
the plugin); they are skipped when it is not installed. validate_plan.py itself
is stdlib only and always tested.

Fixtures under testdata/contracts/ are copied verbatim from the
sebenza-improvements worktree (crates/common/testdata/contracts/) and must not
be edited here: they are the agreed contract.
"""

import copy
import contextlib
import glob
import io
import json
import os
import shutil
import tempfile
import unittest

import validate_plan

try:
    import jsonschema
except ImportError:  # pragma: no cover - dev-time tool
    jsonschema = None

HERE = os.path.dirname(os.path.abspath(__file__))
PLUGIN = os.path.dirname(HERE)
REPO = os.path.dirname(PLUGIN)
SCHEMAS = os.path.join(PLUGIN, "shared", "schemas")
CONTRACTS = os.path.join(HERE, "testdata", "contracts")


def load(path):
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def plan_with(*tasks, phase_extra=None):
    phase = {"id": "phase-1", "name": "P1", "status": "doing", "blocked_reason": None,
             "tasks": list(tasks)}
    phase.update(phase_extra or {})
    return {"$schema": "sebenza-plan-v1", "track_id": "trk_20260101", "phases": [phase]}


def task(tid, status="backlog", **extra):
    t = {"id": tid, "name": tid, "status": status}
    t.update(extra)
    return t


def tracks_with(**track_extra):
    track = {"track_id": "trk_20260101", "type": "feature", "description": "d", "status": "doing",
             "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z",
             "phases_summary": [], "progress": {"total_tasks": 0, "completed_tasks": 0,
                                                "percentage": 0}}
    track.update(track_extra)
    return {"$schema": "sebenza-tracks-v1", "version": "1.0",
            "project": {"name": "p", "path": "/x", "registered_at": "2026-01-01T00:00:00Z"},
            "tracks": [track]}


@unittest.skipIf(jsonschema is None, "jsonschema not installed (dev-time tool)")
class Schemas(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.plan_schema = load(os.path.join(SCHEMAS, "plan.schema.json"))
        cls.tracks_schema = load(os.path.join(SCHEMAS, "tracks.schema.json"))

    def plan_ok(self, doc):
        return not list(jsonschema.Draft202012Validator(self.plan_schema).iter_errors(doc))

    def tracks_ok(self, doc):
        return not list(jsonschema.Draft202012Validator(self.tracks_schema).iter_errors(doc))

    def test_ts16_origin_is_accepted_and_an_absolute_worktree_path_is_not(self):
        origin = {"inbox_item_id": "01M6PA4W07DGMGGAAD3W2FQ301", "worktree": "agent-extensions-skills",
                  "branch": "agent-extensions-skills"}
        self.assertTrue(self.tracks_ok(tracks_with(origin=origin)))
        bad = dict(origin, worktree="/home/someone/worktrees/x")
        self.assertFalse(self.tracks_ok(tracks_with(origin=bad)))
        self.assertFalse(self.tracks_ok(tracks_with(origin={"worktree": "x"})))

    def test_ts18_each_depends_on_scope_is_accepted(self):
        doc = plan_with(task("a"), task("b", depends_on=["a", "trk_20260101/a",
                                                          "@feature-x:trk_20260101/a",
                                                          "@feat/x:a"]))
        self.assertTrue(self.plan_ok(doc))

    def test_ts69_golden_fixture_passes_and_bad_refs_are_rejected(self):
        self.assertTrue(self.plan_ok(load(os.path.join(CONTRACTS, "plan.depends_on.json"))))
        for bad in ("a/b/c", "@:x", "x" * 65, "", "@b", "@b:", "t/", "/k", "has space",
                    "a/../b", "..", "trk/.."):
            doc = plan_with(task("a"), task("b", depends_on=[bad]))
            self.assertFalse(self.plan_ok(doc), bad)

    def test_ts23_current_phase_id_is_optional(self):
        self.assertTrue(self.tracks_ok(tracks_with()))
        self.assertTrue(self.tracks_ok(tracks_with(current_phase_id="phase-2")))

    def test_ts68_request_shape(self):
        good = {"request_id": None, "status": "unsent", "raised_at": "2026-11-05T00:00:00Z"}
        for status in ("open", "unsent", "resolved"):
            req = dict(good, status=status)
            self.assertTrue(self.plan_ok(plan_with(task("a", request=req))), status)
            self.assertTrue(self.plan_ok(plan_with(task("a"), phase_extra={"request": req})))
            self.assertTrue(self.tracks_ok(tracks_with(request=req)))
        self.assertFalse(self.plan_ok(plan_with(task("a", request=dict(good, status="pending")))))
        self.assertFalse(self.plan_ok(plan_with(task("a", request=dict(good, raised_at="yesterday")))))
        self.assertFalse(self.tracks_ok(tracks_with(request=dict(good, status="nope"))))

    def test_ts54_every_existing_plan_and_tracks_file_still_validates(self):
        plans = glob.glob(os.path.join(REPO, ".ai", "sebenza", "tracks", "*", "plan.json"))
        tracks = [os.path.join(REPO, ".ai", "sebenza", "tracks.json")]
        legacy = os.path.join(HERE, "testdata", "legacy")
        plans += glob.glob(os.path.join(legacy, "*plan*.json"))
        tracks += glob.glob(os.path.join(legacy, "*tracks*.json"))
        self.assertGreaterEqual(len(plans), 2)
        for path in plans:
            self.assertTrue(self.plan_ok(load(path)), path)
        for path in tracks:
            if os.path.exists(path):
                self.assertTrue(self.tracks_ok(load(path)), path)

    def test_ts51_unknown_keys_survive_a_read_modify_write(self):
        doc = plan_with(task("a", future_field={"x": 1}), phase_extra={"future": True})
        doc["future_top"] = [1, 2]
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "plan.json")
            with open(path, "w") as fh:
                json.dump(doc, fh)
            import inbox
            data = inbox.read_json(path)
            data["phases"][0]["tasks"][0]["status"] = "doing"
            inbox.write_json_atomic(path, data)
            after = load(path)
        expected = copy.deepcopy(doc)
        expected["phases"][0]["tasks"][0]["status"] = "doing"
        self.assertEqual(after, expected)


class DepRefGrammar(unittest.TestCase):
    def test_splits_like_the_server(self):
        p = validate_plan.parse_ref
        self.assertEqual(p("phase-1-task-1"), (None, None, "phase-1-task-1"))
        self.assertEqual(p("other_track_20261105/phase-2-task-1"),
                         (None, "other_track_20261105", "phase-2-task-1"))
        self.assertEqual(p("@sebenza-improvements:inbox_graph_notes_20261105/phase-2-task-2"),
                         ("sebenza-improvements", "inbox_graph_notes_20261105", "phase-2-task-2"))
        self.assertEqual(p("@feat/x:t/k"), ("feat/x", "t", "k"))
        for bad in ("", "@:k", "@b:", "t/", "/k", "a/../b", "has space", "@b", "x/y/z", "..",
                    "x" * 65, None, 3):
            self.assertIsNone(p(bad), bad)


class Validator(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="sebenza-validate-")
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.ws = os.path.join(self.tmp, ".ai", "sebenza")
        os.makedirs(self.ws)

    def write_plan(self, plan, track_id=None):
        track_id = track_id or plan["track_id"]
        d = os.path.join(self.ws, "tracks", track_id)
        os.makedirs(d, exist_ok=True)
        plan = dict(plan, track_id=track_id)
        path = os.path.join(d, "plan.json")
        with open(path, "w") as fh:
            json.dump(plan, fh)
        return path

    def write_tracks(self, *tracks):
        with open(os.path.join(self.ws, "tracks.json"), "w") as fh:
            json.dump({"$schema": "sebenza-tracks-v1", "version": "1.0",
                       "project": {"name": "p", "path": self.tmp, "registered_at": "x"},
                       "tracks": list(tracks)}, fh)

    def run_main(self, *args):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            code = validate_plan.main(list(args))
        return code, json.loads(out.getvalue())

    def test_ts19_self_reference_cycle_and_dangling_are_errors(self):
        path = self.write_plan(plan_with(
            task("a", depends_on=["a"]),
            task("b", depends_on=["c"]), task("c", depends_on=["trk_20260101/b"]),
            task("d", depends_on=["missing"])))
        code, out = self.run_main(path)
        self.assertEqual(code, 1)
        kinds = sorted(e["kind"] for e in out["errors"])
        self.assertEqual(kinds, ["cycle", "dangling", "self"])
        self.assertFalse(out["ok"])

    def test_malformed_ref_is_an_error(self):
        code, out = self.run_main(self.write_plan(plan_with(task("a", depends_on=["x/y/z"]))))
        self.assertEqual(code, 1)
        self.assertEqual(out["errors"][0]["kind"], "malformed")

    def test_ts20_unresolved_cross_track_is_a_warning_and_exit_0(self):
        path = self.write_plan(plan_with(task("a", depends_on=["other_20260101/x"])))
        code, out = self.run_main(path)
        self.assertEqual(code, 0)
        self.assertTrue(out["ok"])
        self.assertEqual(out["warnings"][0]["kind"], "unresolved")

    def test_cross_track_resolves_within_the_workspace(self):
        self.write_plan(plan_with(task("x", "done")), track_id="other_20260101")
        path = self.write_plan(plan_with(task("a", depends_on=["other_20260101/x"])))
        code, out = self.run_main(path)
        self.assertEqual((code, out["warnings"]), (0, []))

    def test_cross_track_cycle_is_found_in_workspace_mode(self):
        self.write_plan(plan_with(task("x", depends_on=["trk_20260101/a"])), track_id="other_20260101")
        self.write_plan(plan_with(task("a", depends_on=["other_20260101/x"])))
        self.write_tracks({"track_id": "trk_20260101", "plan_path": "./tracks/trk_20260101/plan.json"},
                          {"track_id": "other_20260101",
                           "plan_path": "./tracks/other_20260101/plan.json"})
        code, out = self.run_main("--workspace", self.ws)
        self.assertEqual(code, 1)
        self.assertEqual([e["kind"] for e in out["errors"]], ["cycle"])

    def test_sibling_refs_are_skipped_not_checked(self):
        path = self.write_plan(plan_with(task("a", depends_on=["@other-branch:t_20260101/x"])))
        code, out = self.run_main(path)
        self.assertEqual((code, out["errors"], out["warnings"]), (0, [], []))
        self.assertEqual(out["skipped"], [{"item": "trk_20260101/a",
                                           "ref": "@other-branch:t_20260101/x"}])

    def test_golden_fixture_only_its_dangling_ref_is_an_error(self):
        path = self.write_plan(load(os.path.join(CONTRACTS, "plan.depends_on.json")))
        code, out = self.run_main(path)
        self.assertEqual(code, 1)
        self.assertEqual([(e["kind"], e["ref"]) for e in out["errors"]], [("dangling", "no-such-task")])
        self.assertEqual([w["ref"] for w in out["warnings"]], ["other_track_20261105/phase-2-task-1"])

    def test_unknown_current_phase_id_is_a_warning(self):
        self.write_plan(plan_with(task("a")))
        self.write_tracks({"track_id": "trk_20260101", "plan_path": "./tracks/trk_20260101/plan.json",
                           "current_phase_id": "phase-9"})
        code, out = self.run_main("--workspace", self.ws)
        self.assertEqual(code, 0)
        self.assertEqual([w["kind"] for w in out["warnings"]], ["current_phase_id"])

    def test_workspace_without_tracks_json_and_bad_inputs(self):
        code, out = self.run_main("--workspace", self.ws)
        self.assertEqual((code, out["errors"]), (0, []))
        code, out = self.run_main(os.path.join(self.tmp, "nope.json"))
        self.assertEqual(code, 2)
        bad = os.path.join(self.tmp, "bad.json")
        with open(bad, "w") as fh:
            fh.write("{not json")
        code, out = self.run_main(bad)
        self.assertEqual(code, 2)
        code, out = self.run_main()
        self.assertEqual(code, 2)

    def test_legacy_plan_without_depends_on_is_clean(self):
        code, out = self.run_main(self.write_plan(plan_with(task("a"), task("b"))))
        self.assertEqual((code, out["errors"], out["warnings"]), (0, [], []))

    def test_this_repository_validates(self):
        code, out = self.run_main("--workspace", os.path.join(REPO, ".ai", "sebenza"))
        self.assertEqual(out["errors"], [])
        self.assertEqual(code, 0)


if __name__ == "__main__":
    unittest.main()
