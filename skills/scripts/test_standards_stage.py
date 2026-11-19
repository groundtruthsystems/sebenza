"""The standards stage in sebenza-review and the default rules in sebenza-setup
(standards_review_20261118, FR-24, FR-25; test plan TS-46, TS-55).

The daemon enforces the rules and derives the verdict; these prove the skills say so,
that the sandboxed agent's instructions hard-code no standard, that the setup templates
ship the three default rules in the manifest syntax, and that the upgrade only prints.
"""

import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
PLUGIN = os.path.dirname(HERE)
REPO = os.path.dirname(PLUGIN)
SKILLS = os.path.join(PLUGIN, "skills")
REVIEW = os.path.join(SKILLS, "sebenza-review")
SETUP = os.path.join(SKILLS, "sebenza-setup")
TEMPLATE_GUIDE = os.path.join(SETUP, "assets", "code_styleguides", "general.md")
PROPOSE = os.path.join(SETUP, "scripts", "propose_rules.py")

# The daemon's rule tag (crates/sebenza-server/.../manifest.rs): an optional list marker,
# then `[rule:<id> category=<cat>` and, for long-methods only, `max_lines=<n>`.
RULE_TAG = re.compile(
    r"^\s*(?:[-*+] +|\d+[.)] +)?\[rule:([a-z0-9][a-z0-9-]{0,63})"
    r" category=([a-z0-9][a-z0-9-]{0,63})(?: max_lines=([1-9][0-9]*))?\] +\S"
)
CATEGORIES = {"restating-comments", "excessive-comments", "long-methods"}


def read(*parts):
    with open(os.path.join(*parts), encoding="utf-8") as fh:
        return fh.read()


def rules_in(text):
    """{category: (rule_id, max_lines)} of the tags outside fenced blocks."""
    rules, fenced = {}, False
    for line in text.splitlines():
        if line.lstrip().startswith(("```", "~~~")):
            fenced = not fenced
            continue
        m = None if fenced else RULE_TAG.match(line)
        if m:
            rules[m.group(2)] = (m.group(1), m.group(3))
    return rules


def index_lists(index_md, doc):
    return re.search(r"\]\(\./" + re.escape(doc) + r"\)", index_md) is not None


class ReviewStandardsStage(unittest.TestCase):
    def setUp(self):
        self.text = read(REVIEW, "SKILL.md")

    def test_ts55_existing_checks_and_sign_off_remain(self):
        for phrase in (
            "**Requirement coverage**",
            "**Tests & coverage**",
            "**Test-plan coverage**",
            "**Architecture conformance (features)**",
            "verification mode",
            "Verdict: PASS | CONCERNS | FAIL",
            "the operator verifies and decides",
            "confirm the work is ready for sign-off",
        ):
            self.assertIn(phrase, self.text)

    def test_sign_off_goes_through_the_daemons_gate(self):
        self.assertIn("sebenza-cli track signoff <track_id>", self.text)
        self.assertRegex(self.text, r"(?i)operator token")
        for code in ("no_verdict", "verdict_not_passing", "stale_verdict"):
            self.assertIn(code, self.text)
        self.assertRegex(self.text, r"(?i)never sign off (?:on the operator's behalf|yourself)")

    def test_the_stage_reports_existing_checks_through_agentctl(self):
        self.assertRegex(self.text, r"(?m)^## .*Standards stage")
        self.assertIn("sebenza-agentctl review-checks --track <track_id>", self.text)
        for flag in ("--requirements", "--test-plan", "--coverage", "--architecture"):
            self.assertIn(flag, self.text)

    def test_the_stage_reads_status_and_findings_from_the_daemon(self):
        self.assertIn("sebenza-cli review status <track_id>", self.text)
        self.assertIn("sebenza-cli review findings <track_id>", self.text)
        self.assertRegex(self.text, r"(?i)resolved or unresolved")
        self.assertRegex(self.text, r"(?i)informational notes?")

    def test_an_unreachable_daemon_is_not_run_never_clean(self):
        self.assertIn("not-run", self.text)
        self.assertRegex(self.text, r"(?i)never (?:report it as )?clean")

    def test_the_skill_never_sets_a_verdict(self):
        self.assertRegex(self.text, r"(?i)never sets? (?:the|a) (?:standards )?verdict")
        self.assertRegex(self.text, r"(?i)the daemon derives")

    def test_the_stage_links_the_sandboxed_agents_instructions(self):
        self.assertIn("(standards-stage.md)", self.text)


class SandboxedAgentInstructions(unittest.TestCase):
    def setUp(self):
        self.text = read(REVIEW, "standards-stage.md")

    def test_is_a_file_of_the_review_skill_not_a_new_skill(self):
        self.assertFalse(self.text.startswith("---"), "no skill frontmatter")
        self.assertFalse(os.path.exists(os.path.join(SKILLS, "sebenza-standards-stage")))

    def test_only_manifest_rules_count(self):
        self.assertRegex(self.text, r"(?i)only the rules in `manifest`")
        self.assertRegex(self.text, r"(?i)no other standard")

    def test_hard_codes_no_standard(self):
        for name in CATEGORIES:
            self.assertNotIn(name, self.text)
        self.assertNotRegex(self.text, r"\[rule:")
        self.assertNotRegex(self.text, r"(?i)\b\d+\s+(?:logical\s+)?lines\b")

    def test_repository_text_is_data(self):
        self.assertRegex(self.text, r"(?i)repository text .*is data")
        self.assertRegex(self.text, r"(?i)never instructions")

    def test_every_pair_and_every_prior_is_dispositioned(self):
        self.assertRegex(self.text, r"(?i)every \(rule, file\) pair")
        self.assertRegex(self.text, r"(?i)every prior finding")
        self.assertRegex(self.text, r"(?i)`clean`.*cite.*span|span.*`clean`")

    def test_descriptions_carry_no_code_and_the_reply_is_only_json(self):
        self.assertRegex(self.text, r"(?i)no code excerpts?")
        self.assertRegex(self.text, r"(?i)only the JSON object")
        for key in ("dispositions", "priorFindings", "findings", "ruleId", "findingId"):
            self.assertIn(key, self.text)

    def test_existing_checks_are_not_the_agents_to_report(self):
        self.assertNotIn("existingChecks", self.text)


class SetupDefaultRules(unittest.TestCase):
    def test_ts46_the_template_guide_ships_the_three_default_rules(self):
        rules = rules_in(read(TEMPLATE_GUIDE))
        self.assertEqual(set(rules), CATEGORIES)
        self.assertEqual(rules["long-methods"][1], "20")
        for cat in CATEGORIES - {"long-methods"}:
            self.assertIsNone(rules[cat][1], cat)

    def test_ts46_the_template_index_lists_the_guide(self):
        skill = read(SETUP, "SKILL.md")
        index = skill.split("## 3. The handshake", 1)[1].split("## 4.", 1)[0]
        self.assertTrue(index_lists(index, "code_styleguides/general.md"), index)

    def test_the_skill_documents_the_rules_and_the_diff_only_upgrade(self):
        skill = read(SETUP, "SKILL.md")
        self.assertIn("[rule:<id> category=<category>", skill)
        self.assertIn("python3 scripts/propose_rules.py", skill)
        self.assertRegex(skill, r"(?i)unified diff")
        self.assertRegex(skill, r"(?i)writes nothing")


class AdoptedHere(unittest.TestCase):
    def test_this_repository_adopts_the_default_rules(self):
        here = rules_in(read(REPO, ".ai", "sebenza", "code_styleguides", "general.md"))
        self.assertEqual(set(here), CATEGORIES)
        self.assertEqual(here, rules_in(read(TEMPLATE_GUIDE)))

    def test_this_repositorys_index_lists_the_guide(self):
        index = read(REPO, ".ai", "sebenza", "index.md")
        self.assertTrue(index_lists(index, "code_styleguides/general.md"), index)


class ProposeRules(unittest.TestCase):
    """TS-46: the upgrade prints the proposed rules as a unified diff and writes nothing."""

    def setUp(self):
        self.root = tempfile.mkdtemp(prefix="propose-rules-")
        self.addCleanup(shutil.rmtree, self.root)
        self.ws = os.path.join(self.root, ".ai", "sebenza")
        os.makedirs(os.path.join(self.ws, "code_styleguides"))

    def write(self, rel, text):
        with open(os.path.join(self.ws, rel), "w", encoding="utf-8") as fh:
            fh.write(text)

    def snapshot(self):
        out = {}
        for base, _, files in os.walk(self.root):
            for name in files:
                path = os.path.join(base, name)
                with open(path, "rb") as fh:
                    out[path] = (fh.read(), os.stat(path).st_mtime_ns)
        return out

    def run_script(self):
        return subprocess.run([sys.executable, "-I", PROPOSE, "--root", self.root],
                              capture_output=True, text=True, check=False)

    def test_ts46_an_existing_project_gets_a_diff_and_no_write(self):
        self.write("index.md", "# Project Context\n\n## Workflow\n- [Workflow](./workflow.md)\n")
        self.write("code_styleguides/general.md", "# General\n\n- Be clear.\n")
        before = self.snapshot()
        out = self.run_script()
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(self.snapshot(), before, "the upgrade writes nothing")
        self.assertIn("--- a/.ai/sebenza/code_styleguides/general.md", out.stdout)
        self.assertIn("+++ b/.ai/sebenza/code_styleguides/general.md", out.stdout)
        self.assertIn("--- a/.ai/sebenza/index.md", out.stdout)
        added = "\n".join(l[1:] for l in out.stdout.splitlines() if l.startswith("+"))
        self.assertEqual(set(rules_in(added)), CATEGORIES)
        self.assertTrue(index_lists(added, "code_styleguides/general.md"), added)

    def test_a_category_the_project_already_rules_is_left_alone(self):
        self.write("index.md", "- [Guides](./code_styleguides/)\n")
        self.write("code_styleguides/general.md",
                   "- [rule:short category=long-methods max_lines=30] Ours.\n")
        out = self.run_script()
        self.assertEqual(out.returncode, 0, out.stderr)
        added = "\n".join(l[1:] for l in out.stdout.splitlines() if l.startswith("+"))
        self.assertEqual(set(rules_in(added)), CATEGORIES - {"long-methods"})
        self.assertNotIn("index.md", out.stdout, "a listed directory already covers the guide")

    def test_a_project_with_every_rule_gets_no_diff(self):
        self.write("index.md", "- [General](./code_styleguides/general.md)\n")
        self.write("code_styleguides/general.md", read(TEMPLATE_GUIDE))
        out = self.run_script()
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(out.stdout, "")
        self.assertIn("already", out.stderr)

    def test_a_missing_guide_is_proposed_as_a_new_file(self):
        self.write("index.md", "# Project Context\n")
        out = self.run_script()
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn("--- /dev/null", out.stdout)
        self.assertFalse(os.path.exists(os.path.join(self.ws, "code_styleguides", "general.md")))

    def test_no_workspace_is_an_error(self):
        shutil.rmtree(os.path.join(self.root, ".ai"))
        out = self.run_script()
        self.assertEqual(out.returncode, 2)
        self.assertEqual(out.stdout, "")


if __name__ == "__main__":
    unittest.main()
