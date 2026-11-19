"""The writing guidance, trailers and edit boundary in sebenza-implement and sebenza-plan
(standards_review_20261118, FR-23; test plan TS-41, TS-42, TS-43).

These prove the words are there and that the implement text hard-codes no threshold; the
daemon, not the skill, enforces scope and the project's own rules.
"""

import os
import re
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SKILLS = os.path.join(os.path.dirname(HERE), "skills")


def skill(name):
    with open(os.path.join(SKILLS, name, "SKILL.md"), encoding="utf-8") as fh:
        return fh.read()


# A number tied to a unit of code size, or a bound phrased around a number.
NUMERIC_LIMIT = re.compile(
    r"(?i)\b\d+\s*(?:-\s*)?(?:lines?|loc|statements?|characters?|chars?|columns?|words?|"
    r"comments?|methods?|functions?|params?|parameters?|levels?|percent|%)\b"
    r"|\b(?:at most|no more than|fewer than|less than|under|max(?:imum)?|limit(?:ed)? (?:of|to))\s+\d+"
)


class ImplementGuidance(unittest.TestCase):
    def setUp(self):
        self.text = skill("sebenza-implement")

    def test_ts41_comment_only_for_non_obvious_intent_on_new_and_changed_code(self):
        self.assertRegex(self.text, r"(?i)comment only to explain intent that isn't obvious")
        self.assertRegex(self.text, r"(?i)never restate what the code does")
        self.assertRegex(self.text, r"(?i)apply this to new and changed code only")
        self.assertRegex(self.text, r"(?i)do not remove\s+comments from untouched code")

    def test_ts42_short_methods_and_a_named_method_instead_of_a_comment(self):
        self.assertRegex(self.text, r"(?i)keep methods short")
        self.assertRegex(self.text, r"(?i)extract a\s+well-named method\*?\*? instead of writing that comment")

    def test_ts43_trailers_boundary_and_no_hard_coded_threshold(self):
        self.assertIn("Sebenza-Track: <track_id>", self.text)
        self.assertIn("Sebenza-Task: <task_id>", self.text)
        self.assertRegex(self.text, r"(?i)add both trailers to every commit")
        self.assertRegex(self.text, r"(?i)stay inside the task's edit boundary")
        self.assertRegex(self.text, r"`files`")
        self.assertRegex(self.text, r"(?i)project's own `\.ai/sebenza/` documents")
        self.assertIsNone(NUMERIC_LIMIT.search(self.text), NUMERIC_LIMIT.search(self.text))

    def test_completion_goes_through_the_daemon(self):
        self.assertIn("sebenza-agentctl complete --track <track_id>", self.text)
        self.assertRegex(self.text, r"(?i)never write the track's `status: \"done\"` yourself")
        self.assertNotRegex(self.text, r'Set the track `status` to `"done"`')

    def test_the_guidance_never_produces_a_finding_by_itself(self):
        self.assertRegex(self.text, r"(?i)never produce a review finding by themselves")


class NumericLimitPattern(unittest.TestCase):
    def test_it_catches_thresholds_and_ignores_exit_codes_and_section_numbers(self):
        for bad in ("keep methods under 20 lines", "at most 3 comments", "max 40", "80 chars"):
            self.assertIsNotNone(NUMERIC_LIMIT.search(bad), bad)
        for fine in ("Exit `7` means", "see §5 step 3", "TS-41", "version 1.4"):
            self.assertIsNone(NUMERIC_LIMIT.search(fine), fine)


class PlanBoundary(unittest.TestCase):
    def setUp(self):
        self.text = skill("sebenza-plan")

    def test_tasks_get_a_files_boundary(self):
        self.assertRegex(self.text, r"(?i)give every task a `files` edit boundary")
        self.assertRegex(self.text, r'"files": \[')

    def test_operator_approval_is_followed_by_the_boundary_snapshot(self):
        self.assertIn("`[approval]` request", self.text)
        self.assertIn("sebenza-cli track approve-plan <track_id>", self.text)
        self.assertRegex(self.text, r"(?i)operator token")


if __name__ == "__main__":
    unittest.main()
