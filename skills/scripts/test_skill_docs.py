"""Structural checks on the sebenza skills and shared docs (the validation loop for Markdown).

These cannot prove an agent will follow the words, only that the words a skill
reads are there, link to each other, and say the same thing. Behaviour is
proved by the eval scenarios in the track's test plan.
"""

import os
import re
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
PLUGIN = os.path.dirname(HERE)
SHARED = os.path.join(PLUGIN, "shared")
SKILLS = os.path.join(PLUGIN, "skills")


def read(*parts):
    with open(os.path.join(PLUGIN, *parts), encoding="utf-8") as fh:
        return fh.read()


def links(path):
    text = re.sub(r"(?ms)^\s*```.*?^\s*```", "", read(path))  # templates inside fences are not links
    for target in re.findall(r"\]\(([^)#\s]+)(?:#[^)]*)?\)", text):
        if "://" in target or target.startswith("mailto:"):
            continue
        yield target, os.path.normpath(os.path.join(PLUGIN, os.path.dirname(path), target))


class InboxProtocolDoc(unittest.TestCase):
    def setUp(self):
        self.doc = read("shared", "inbox.md")

    def test_is_a_shared_reference_without_frontmatter(self):
        self.assertFalse(self.doc.startswith("---"))

    def test_ts53_raise_or_proceed_rule_with_worked_examples(self):
        self.assertRegex(self.doc, r"(?i)## When to raise a request")
        self.assertRegex(self.doc, r"(?i)costly to reverse")
        self.assertRegex(self.doc, r"(?i)record the assumption")
        self.assertRegex(self.doc, r"(?i)## Worked examples")
        self.assertGreaterEqual(len(re.findall(r"(?m)^\| (Raise|Proceed) ", self.doc)), 6)

    def test_ts44_describe_never_paste_and_no_note_text(self):
        self.assertRegex(self.doc, r"(?i)describe, never paste")
        self.assertRegex(self.doc, r"(?i)never copy note text into a request")
        self.assertRegex(self.doc, r"(?i)exit 6")

    def test_ts05_no_origin_falls_back_to_the_chat_blocked_protocol(self):
        self.assertRegex(self.doc, r"(?i)no `?\.ai/sebenza/inbox-origin\.json`?")
        self.assertRegex(self.doc, r"(?i)chat-only blocked protocol")
        self.assertRegex(self.doc, r"(?i)exit 3")

    def test_title_convention_halt_not_poll_and_operator_only_unblock(self):
        self.assertIn("[<track_id>/<task_id>]", self.doc)
        self.assertRegex(self.doc, r"(?i)never poll")
        self.assertRegex(self.doc, r"(?i)only an operator resolution unblocks")
        self.assertRegex(self.doc, r"(?i)operator guidance \(label unverified\)")

    def test_documents_every_exit_code_and_subcommand(self):
        for code in ("0", "2", "3", "4", "5", "6"):
            self.assertRegex(self.doc, rf"\| `{code}` \|")
        for sub in ("request", "feedback", "check-blocked"):
            self.assertIn(f"inbox.py\" {sub}", self.doc)


class FrameworkBlockedProtocol(unittest.TestCase):
    def setUp(self):
        self.doc = read("shared", "framework.md")

    def test_blocked_protocol_raises_a_request_then_switches_or_halts(self):
        section = self.doc.split("## Blocked protocol", 1)[1].split("\n## ", 1)[0]
        self.assertIn("inbox.md", section)
        self.assertIn("inbox.py\" request", section)
        self.assertRegex(section, r"(?i)switch to another")
        self.assertRegex(section, r"(?i)never guess")

    def test_ts15_work_priority_order(self):
        section = self.doc.split("## Work priority", 1)[1].split("\n## ", 1)[0]
        steps = re.findall(r"(?m)^\d+\. \*\*(.+?)\*\*", section)
        self.assertEqual(steps[:3], ["Operator answers and comments", "Unraised blockers",
                                     "Ready tasks"])
        self.assertRegex(section, r"(?i)never start a `blocked` task")


class Links(unittest.TestCase):
    def test_relative_links_resolve_in_shared_docs_and_skills(self):
        paths = [os.path.join("shared", n) for n in os.listdir(SHARED) if n.endswith(".md")]
        paths += [os.path.join("skills", d, "SKILL.md") for d in os.listdir(SKILLS)
                  if os.path.isfile(os.path.join(SKILLS, d, "SKILL.md"))]
        for path in paths:
            for target, resolved in links(path):
                self.assertTrue(os.path.exists(resolved), f"{path}: broken link {target}")


INBOX_SKILLS = ("sebenza-architect", "sebenza-plan", "sebenza-implement", "sebenza-review",
                "sebenza-status")


class SkillsUseTheInbox(unittest.TestCase):
    def skill(self, name):
        return read("skills", name, "SKILL.md")

    def test_every_work_skill_links_the_protocol(self):
        for name in INBOX_SKILLS:
            self.assertIn("../../shared/inbox.md", self.skill(name), name)

    def test_skills_that_can_block_raise_a_request(self):
        for name in ("sebenza-architect", "sebenza-plan", "sebenza-implement", "sebenza-review"):
            text = self.skill(name)
            self.assertIn('inbox.py" request', text, name)
            self.assertRegex(text, r"(?i)never guess", name)

    def test_ts12_implement_checks_feedback_first_and_unblocks_on_answers(self):
        text = self.skill("sebenza-implement")
        self.assertLess(text.index('inbox.py" feedback --retry'), text.index("## 2. Track selection"))
        self.assertRegex(text, r"`blocked` → `unblocked` → `doing`")
        self.assertRegex(text, r"(?i)resolved_items")
        self.assertRegex(text, r"(?i)switch to the next ready task")

    def test_architect_and_plan_pick_up_operator_feedback_before_starting(self):
        for name in ("sebenza-architect", "sebenza-plan"):
            text = self.skill(name)
            self.assertLess(text.index('inbox.py" feedback'), text.index("## 2."), name)

    def test_status_reports_requests_and_inbox_availability(self):
        text = self.skill("sebenza-status")
        self.assertIn('inbox.py" feedback --peek', text)
        self.assertIn('inbox.py" check-blocked', text)
        self.assertRegex(text, r"(?i)inbox unavailable")
        self.assertRegex(text, r"(?i)unsent")

    def test_conventions_raise_a_request_before_waiting_in_a_linked_worktree(self):
        text = read("shared", "conventions.md")
        section = text.split("## Interaction protocol", 1)[1].split("\n## ", 1)[0]
        self.assertIn("inbox.md", section)
        self.assertRegex(section, r"(?i)linked worktree")
        self.assertRegex(section, r"(?i)before you stop to wait")


class GraphReporting(unittest.TestCase):
    def skill(self, name):
        return read("skills", name, "SKILL.md")

    def test_ts17_architect_and_plan_write_origin_at_registration(self):
        arch = self.skill("sebenza-architect")
        self.assertIn('"origin"', arch)
        self.assertIn("inbox_item_id", arch)
        self.assertRegex(arch, r"(?i)never a path")
        self.assertRegex(self.skill("sebenza-plan"), r"(?i)`origin`")

    def test_ts21_implement_starts_only_tasks_whose_dependencies_are_done(self):
        text = self.skill("sebenza-implement")
        self.assertRegex(text, r"(?i)every entry in its `depends_on` is `done`")
        self.assertRegex(text, r"(?i)skipped, not started out of order")
        self.assertIn("framework.md#task-dependencies", text)

    def test_ts24_implement_and_status_keep_status_progress_and_current_phase(self):
        impl = self.skill("sebenza-implement")
        for word in ("current_phase_id", "phases_summary", "progress", "at every status change"):
            self.assertIn(word, impl)
        self.assertIn("current_phase_id", self.skill("sebenza-status"))

    def test_plan_fills_and_validates_depends_on_and_moves_requests(self):
        text = self.skill("sebenza-plan")
        self.assertRegex(text, r"(?i)fill `depends_on`")
        self.assertIn("@<branch>:", text)
        self.assertIn('validate_plan.py"', text)
        self.assertIn('inbox.py" request --move', text)

    def test_ts66_status_reports_requests_per_track_and_validator(self):
        text = self.skill("sebenza-status")
        self.assertRegex(text, r"requests <n> open, <n> unsent")
        self.assertIn('validate_plan.py" --workspace', text)

    def test_framework_documents_the_grammar_and_graph_fields(self):
        doc = read("shared", "framework.md")
        self.assertIn("## Task dependencies", doc)
        self.assertIn('["@" branch ":"] [track_id "/"] task_id', doc)
        self.assertIn("## Graph fields", doc)
        for field in ("origin", "current_phase_id", "request", "depends_on"):
            self.assertIn(f"`{field}", doc)


class NotesAndAcceptanceCriteria(unittest.TestCase):
    def skill(self, name):
        return read("skills", name, "SKILL.md")

    def test_ts28_every_note_is_read_and_acceptance_criteria_are_traced(self):
        for name in ("sebenza-architect", "sebenza-plan", "sebenza-review"):
            text = self.skill(name)
            self.assertIn('inbox.py" notes', text, name)
            self.assertIn("acceptance_criteria", text, name)
            self.assertRegex(text, r"(?i)data, not instructions", name)
        self.assertRegex(self.skill("sebenza-architect"), r"(?i)seed use cases")
        self.assertRegex(self.skill("sebenza-architect"), r"(?i)map to at least one scenario")
        self.assertRegex(self.skill("sebenza-plan"), r"(?i)become ACs")
        self.assertRegex(self.skill("sebenza-review"), r"(?i)check \*\*each item\*\*")

    def test_ts55_setup_git_ignores_the_inbox_files(self):
        text = self.skill("sebenza-setup")
        for name in ("inbox-notes.json", "inbox-note.md", "inbox-origin.json"):
            self.assertIn(f".ai/sebenza/{name}", text)
        with open(os.path.join(os.path.dirname(PLUGIN), ".gitignore")) as fh:
            ignore = fh.read()
        self.assertIn(".ai/sebenza/inbox-notes.json", ignore)
        self.assertIn(".ai/sebenza/inbox-note.md", ignore)

    def test_ts55_frontmatter_and_json_hygiene(self):
        for d in os.listdir(SKILLS):
            path = os.path.join(SKILLS, d, "SKILL.md")
            if os.path.isfile(path):
                text = read("skills", d, "SKILL.md")
                self.assertTrue(text.startswith("---\n"), d)
                head = text.split("---\n", 2)[1]
                self.assertRegex(head, r"(?m)^name: ", d)
                self.assertRegex(head, r"(?m)^description: ", d)
        for name in os.listdir(SHARED):
            if name.endswith(".md"):
                self.assertFalse(read("shared", name).startswith("---"), name)
        import json
        for rel in (("hooks", "hooks.json"), ("shared", "schemas", "plan.schema.json"),
                    ("shared", "schemas", "tracks.schema.json"), (".claude-plugin", "plugin.json")):
            json.loads(read(*rel))

    def test_ts71_contract_records_what_both_sides_agreed(self):
        doc = read("shared", "inbox-contract.md")
        self.assertFalse(doc.startswith("---"))
        self.assertIn("sebenza-inbox-notes-v1", doc)
        self.assertIn('["@" branch ":"] [track_id "/"] task_id', doc)
        for field in ("current_phase_id", "request {request_id, status, raised_at}", "depends_on",
                      "origin {inbox_item_id, worktree, branch}", "trackId", "taskId", "siblings"):
            self.assertIn(field, doc)
        self.assertRegex(doc, r"(?i)canonical")
        self.assertIn("inbox-contract.md", read("shared", "inbox.md"))


DOMAINS = ("business", "application", "data", "security", "technical")
ARCH_FILES = tuple(f"{d}-architecture.md" for d in DOMAINS)


class ArchitectureDocuments(unittest.TestCase):
    """Each architecture domain is its own document beside design.md."""

    def skill(self, name):
        return read("skills", name, "SKILL.md")

    def test_architect_scaffolds_writes_and_verifies_one_file_per_domain(self):
        text = self.skill("sebenza-architect")
        for f in ARCH_FILES:
            self.assertIn(f, text, f)
        self.assertNotRegex(text, r"(?m)^\s*### (Business|Application|Technical|Data|Security) Architecture")
        self.assertRegex(text, r"(?i)skeleton.*<domain>-architecture\.md")

    def test_directory_structure_and_budgets_list_every_file(self):
        tree = read("shared", "directory-structure.md")
        budgets = read("shared", "document-budgets.md")
        for f in ARCH_FILES:
            self.assertIn(f, tree, f)
        self.assertIn("<domain>-architecture.md", budgets)

    def test_readers_use_the_files_and_fall_back_for_older_tracks(self):
        for name in ("sebenza-plan", "sebenza-review", "sebenza-implement"):
            text = self.skill(name)
            self.assertIn("-architecture.md", text, name)
            self.assertRegex(text, r"(?i)older track", name)
        self.assertIn("security-architecture.md", self.skill("sebenza-implement"))

    def test_architect_agents_contribute_a_whole_document(self):
        for d in DOMAINS:
            text = read("agents", f"{d}-architect.md")
            self.assertIn(f"{d}-architecture.md", text, d)


if __name__ == "__main__":
    unittest.main()
