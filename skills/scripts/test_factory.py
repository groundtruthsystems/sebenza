"""Software factory, project level: ordered architecture, the approval gate,
independent review and the unlinked fallback (project track TS-25, TS-27,
TS-29; TS-28's request half).

Doc checks read the skills; behaviour checks drive inbox.py against the fake
daemon and fake agentctl in inbox_testkit.
"""

import json
import os
import re
import unittest

from inbox_testkit import TRACK, Workspace, request, run

HERE = os.path.dirname(os.path.abspath(__file__))
PLUGIN = os.path.dirname(HERE)


def read(*parts):
    with open(os.path.join(PLUGIN, *parts), encoding="utf-8") as fh:
        return fh.read()


def section(text, heading):
    m = re.search(rf"(?ms)^## {re.escape(heading)}.*?(?=^## )", text)
    return m.group(0) if m else ""


class ArchitectOrder(unittest.TestCase):
    """TS-25: business, then application and data (with the business document
    and the solution handover), then technical and security, then test."""

    def setUp(self):
        self.skill = read("skills", "sebenza-architect", "SKILL.md")
        self.s4 = section(self.skill, "4. Architecture definition")

    def test_waves_are_in_order(self):
        wave = lambda name: self.s4.find(name)
        for w in ("Wave 1", "Wave 2", "Wave 3"):
            self.assertNotEqual(wave(w), -1, f"{w} missing from §4")
        self.assertLess(wave("Wave 1"), wave("Wave 2"))
        self.assertLess(wave("Wave 2"), wave("Wave 3"))
        w1 = self.s4[wave("Wave 1"):wave("Wave 2")]
        w2 = self.s4[wave("Wave 2"):wave("Wave 3")]
        w3 = self.s4[wave("Wave 3"):]
        self.assertIn("business-architect", w1)
        self.assertNotIn("application-architect", w1)
        for agent in ("application-architect", "data-architect"):
            self.assertIn(agent, w2)
        self.assertIn("business-architecture.md", w2, "wave 2 is given the business document")
        self.assertIn("solution-handover.md", w2, "and the solution handover brief")
        for agent in ("technical-architect", "security-architect"):
            self.assertIn(agent, w3)
        self.assertLess(self.skill.find("## 4."), self.skill.find("## 5. Test plan"), "test stays last")

    def test_design_review_and_approval_follow_verification(self):
        review = section(self.skill, "7a. Independent design review")
        self.assertIn("inbox.py\" review --level design", review)
        self.assertIn("round n of 3", review.lower())
        self.assertIn("--local", review, "unlinked worktrees review in-session")
        self.assertIn("approval --track", review)


class ImplementWaitsForApproval(unittest.TestCase):
    """TS-29 (doc half): the implement skill refuses until approval."""

    def test_implement_checks_the_gate_before_any_task(self):
        skill = read("skills", "sebenza-implement", "SKILL.md")
        gate = section(skill, "3a. Approval gate")
        self.assertIn("approval --track", gate)
        self.assertIn("7", gate, "names the not-approved exit code")
        self.assertRegex(gate, r"(?i)do not (start|implement)")
        self.assertLess(skill.find("## 3a."), skill.find("## 5. Execute tasks"))

    def test_the_protocol_documents_the_new_commands_and_codes(self):
        doc = read("shared", "inbox.md")
        for needle in ("approval --track", "review --level", "--local", "| `7` |", "| `8` |", "| `9` |", "| `10` |"):
            self.assertIn(needle, doc, needle)


class ApprovalGate(unittest.TestCase):
    """TS-29 (behaviour) + TS-28 (request half)."""

    def setUp(self):
        self.w = Workspace()
        os.makedirs(os.path.join(self.w.ws, "tracks", TRACK), exist_ok=True)
        with open(os.path.join(self.w.ws, "tracks", TRACK, "design.md"), "w") as fh:
            fh.write("# Design\n")

    def tearDown(self):
        if self.w.daemon:
            self.w.daemon.close()

    def test_unanswered_approval_refuses_and_answering_unblocks(self):
        code, out, _, _ = run(["approval", "--track", TRACK], self.w.wt)
        self.assertEqual(code, 7)
        self.assertEqual((out["approved"], out["status"]), (False, "missing"))

        code, out, _, _ = run(["approval", "--track", TRACK, "--raise"], self.w.wt)
        self.assertEqual(code, 7, out)
        self.assertEqual(out["status"], "open")
        state = self.w.read_json(self.w.state_path)
        filed = [r for r in state["requests"] if r["title"].startswith(f"[{TRACK}/approval]")]
        self.assertEqual(len(filed), 1, "TS-28: the approval request reaches the origin item")
        # Asking again files nothing new.
        run(["approval", "--track", TRACK, "--raise"], self.w.wt)
        state = self.w.read_json(self.w.state_path)
        self.assertEqual(len([r for r in state["requests"] if "/approval]" in r["title"]]), 1)

        # Another open track request does not count as approval.
        filed[0].update(status="resolved", confirmedText="Approved. Go ahead.")
        self.w.set_state(requests=[request(9, f"[{TRACK}] unrelated", status="open"), filed[0]])
        code, out, _, _ = run(["approval", "--track", TRACK], self.w.wt)
        self.assertEqual(code, 0, out)
        self.assertTrue(out["approved"])
        meta = self.w.read_json(os.path.join(self.w.ws, "tracks", TRACK, "metadata.json"))
        self.assertEqual(len(meta["design_approved"]["design_hash"]), 64)

    def test_unlinked_worktrees_ask_in_chat(self):
        w = Workspace(origin=False)
        try:
            code, out, _, _ = run(["approval", "--track", TRACK], w.wt)
            self.assertEqual(code, 0)
            self.assertEqual((out["linked"], out["approved"]), (False, None))
            self.assertIn("chat", out["hint"])
        finally:
            if w.daemon:
                w.daemon.close()


class Review(unittest.TestCase):
    """TS-26 (skill half) and TS-27: linked reviews go to the daemon; unlinked
    ones are recorded locally, flagged not independent, capped at three."""

    def tearDown(self):
        for w in getattr(self, "spaces", []):
            if w.daemon:
                w.daemon.close()

    def space(self, **kw):
        w = Workspace(**kw)
        self.spaces = getattr(self, "spaces", []) + [w]
        return w

    def test_a_linked_review_passes_the_daemon_verdict_through(self):
        w = self.space()
        w.configure_agentctl(review_stdout={"status": "succeeded", "verdict": "changes", "round": 1, "roundOf": 3,
                                            "findings": [{"problem_id": "F-1", "task": "t1"}]})
        code, out, _, _ = run(["review", "--level", "code", "--track", TRACK], w.wt)
        self.assertEqual(code, 0, out)
        self.assertEqual((out["verdict"], out["round"], out["independent"]), ("changes", 1, True))
        calls = [json.loads(l) for l in open(os.path.join(w.sdir, "agentctl.log"))]
        self.assertEqual(calls[-1]["argv"], ["review", "--level", "code", "--track", TRACK])
        w.configure_agentctl(review_stdout={"status": "refused", "code": "round_limit"}, review_exit=3)
        code, out, _, _ = run(["review", "--level", "code", "--track", TRACK], w.wt)
        self.assertEqual(code, 8, "round_limit")
        w.configure_agentctl(review_stdout={"status": "voided"}, review_exit=5)
        self.assertEqual(run(["review", "--level", "code", "--track", TRACK], w.wt)[0], 10)
        w.configure_agentctl(review_stdout={"code": "not_independent"}, review_exit=4)
        self.assertEqual(run(["review", "--level", "code", "--track", TRACK], w.wt)[0], 9)

    def test_ts27_an_unlinked_review_is_local_and_not_independent(self):
        w = self.space(origin=False)
        code, out, _, _ = run(["review", "--level", "design", "--track", TRACK], w.wt)
        self.assertEqual(code, 3)
        self.assertFalse(out["independent"])
        for n in (1, 2, 3):
            code, out, _, _ = run(["review", "--level", "design", "--track", TRACK, "--local",
                                   "--verdict", "changes", "--findings", "-"], w.wt,
                                  stdin=json.dumps([{"problem_id": "F-1", "title": "x"}]))
            self.assertEqual((code, out["round"], out["independent"]), (0, n, False), out)
        code, out, _, _ = run(["review", "--level", "design", "--track", TRACK, "--local", "--verdict", "agreed"], w.wt)
        self.assertEqual(code, 8, "a 4th local round is refused too")
        rounds = w.read_json(os.path.join(w.ws, "tracks", TRACK, "reviews.json"))["rounds"]
        self.assertTrue(all(r["independent"] is False for r in rounds))


class CodeReviewRounds(unittest.TestCase):
    """TS-30..TS-33: findings reopen the tasks they name; the cap stops the
    loop; final verification carries what is still open."""

    def setUp(self):
        self.w = Workspace()
        self.w.write_tracks(task_status="done")
        self.plan_path = os.path.join(self.w.ws, "tracks", TRACK, "plan.json")

    def tearDown(self):
        self.w.close()

    def plan(self):
        return self.w.read_json(self.plan_path)

    def task(self, tid):
        return next(t for ph in self.plan()["phases"] for t in ph["tasks"] if t["id"] == tid)

    def review(self, verdict, findings, rnd, code=0):
        self.w.configure_agentctl(review_exit=code, review_stdout={
            "status": "succeeded" if code == 0 else "refused", "verdict": verdict, "round": rnd,
            "roundOf": 3, "findings": findings, "code": "round_limit" if code == 3 else None})
        return run(["review", "--level", "code", "--track", TRACK, "--apply"], self.w.wt)

    def test_ts30_findings_reopen_only_the_tasks_they_name(self):
        code, out, _, _ = self.review("changes", [
            {"problem_id": "F-1", "title": "Totals ignore tax", "detail": "src/x.rs:4 sums net. " * 60, "task": "t1"},
            {"problem_id": "F-2", "title": "No such task", "detail": "d", "task": "nope"}], 1)
        self.assertEqual(code, 0, out)
        self.assertEqual(out["reopened"], ["t1"])
        self.assertEqual(out["unmatched"], ["F-2"])
        t1 = self.task("t1")
        self.assertEqual((t1["status"], t1["review_round"]), ("doing", 1))
        self.assertEqual(t1["review_findings"][0]["id"], "F-1")
        self.assertEqual(t1["review_findings"][0]["status"], "open")
        self.assertLessEqual(len(t1["review_findings"][0]["text"]), 500)
        self.assertEqual(self.task("t2")["status"], "backlog", "an unnamed task is unchanged")
        tracks = self.w.read_json(os.path.join(self.w.ws, "tracks.json"))
        self.assertEqual(tracks["tracks"][0]["phases_summary"][0]["status"], "doing")
        # Round 2 agrees: the open finding on t1 is closed, nothing reopened.
        code, out, _, _ = self.review("agreed", [], 2)
        self.assertEqual((code, out["reopened"]), (0, []))
        self.assertEqual(self.task("t1")["review_findings"][0]["status"], "resolved")

    def test_ts31_ts32_the_fourth_round_is_refused_and_findings_carry_forward(self):
        for n in (1, 2, 3):
            code, out, _, _ = self.review("changes", [{"problem_id": "F-1", "title": "Tax", "detail": "d", "task": "t1"}], n)
            self.assertEqual(code, 0, out)
        code, out, _, _ = self.review("changes", [], 4, code=3)
        self.assertEqual(code, 8, "round_limit")
        self.assertEqual(self.task("t1")["review_round"], 3, "nothing applied past the cap")
        code, out, _, _ = run(["verification", "--track", TRACK], self.w.wt)
        self.assertEqual(code, 0, out)
        self.assertEqual(out["open_findings"], 1)
        state = self.w.read_json(self.w.state_path)
        req = [r for r in state["requests"] if r["title"].startswith(f"[{TRACK}/verification]")]
        self.assertEqual(len(req), 1)
        self.assertIn("F-1", req[0]["body"])
        self.assertIn("round 3 of 3", req[0]["body"])

    def test_ts33_acceptance_raises_verification_with_nothing_open(self):
        self.review("changes", [{"problem_id": "F-1", "title": "Tax", "detail": "d", "task": "t1"}], 1)
        self.review("agreed", [], 2)
        code, out, _, _ = run(["verification", "--track", TRACK], self.w.wt)
        self.assertEqual((code, out["open_findings"]), (0, 0), out)
        body = [r for r in self.w.read_json(self.w.state_path)["requests"] if "/verification]" in r["title"]][0]["body"]
        self.assertIn("accepted", body.lower())
        # Raising again files nothing new.
        run(["verification", "--track", TRACK], self.w.wt)
        reqs = [r for r in self.w.read_json(self.w.state_path)["requests"] if "/verification]" in r["title"]]
        self.assertEqual(len(reqs), 1)

    def test_the_review_skill_runs_rounds_and_stops(self):
        skill = read("skills", "sebenza-review", "SKILL.md")
        rounds = section(skill, "Code review rounds")
        for needle in ("review --level code", "--apply", "round n of 3", "verification --track", "8"):
            self.assertIn(needle, rounds, needle)
        self.assertRegex(rounds, r"(?i)stop")


if __name__ == "__main__":
    unittest.main()
