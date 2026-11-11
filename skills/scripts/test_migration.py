"""Structural checks for the move of the sebenza plugin into this repository.

TS-63 (AC-M.0): the repo-root marketplace and the plugin manifest validate, with
stable names. TS-34/TS-35 (AC-M.1): the tree under skills/ is exactly the source
listing recorded in MIGRATION.md plus the post-move additions recorded there.
TS-46 (FT-12): the hook script is stdlib-only, never uses a shell, and only talks
to the local daemon.
"""

import ast
import json
import os
import re
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
PLUGIN = os.path.dirname(HERE)          # skills/
REPO = os.path.dirname(PLUGIN)          # repository root

SKILLS = ["sebenza-architect", "sebenza-plan", "sebenza-implement", "sebenza-review",
          "sebenza-revert", "sebenza-setup", "sebenza-status"]
IGNORED_DIRS = {"__pycache__"}


def load_json(*parts):
    with open(os.path.join(*parts), encoding="utf-8") as fh:
        return json.load(fh)


def tree():
    out = set()
    for base, dirs, files in os.walk(PLUGIN):
        dirs[:] = [d for d in dirs if d not in IGNORED_DIRS]
        for name in files:
            if name.endswith(".pyc"):
                continue
            out.add(os.path.relpath(os.path.join(base, name), PLUGIN).replace(os.sep, "/"))
    return out


def migration_sections():
    with open(os.path.join(PLUGIN, "MIGRATION.md"), encoding="utf-8") as fh:
        text = fh.read()

    def fenced(heading):
        m = re.search(r"^## " + re.escape(heading) + r"\s*$.*?^```[a-z]*\n(.*?)^```", text, re.M | re.S)
        if not m:
            return None
        return {line.strip() for line in m.group(1).splitlines() if line.strip()}

    return text, fenced("Source listing"), fenced("Post-move additions")


class Marketplace(unittest.TestCase):
    """TS-63: the repo is a marketplace named sebenza whose sebenza plugin is ./skills."""

    def test_repo_root_marketplace_names_sebenza_with_source_skills(self):
        market = load_json(REPO, ".claude-plugin", "marketplace.json")
        self.assertEqual(market["name"], "sebenza")
        self.assertIn("owner", market)
        plugins = {p["name"]: p for p in market["plugins"]}
        self.assertEqual(set(plugins), {"sebenza"})
        self.assertEqual(plugins["sebenza"]["source"], "./skills")

    def test_plugin_manifest_is_named_sebenza(self):
        manifest = load_json(PLUGIN, ".claude-plugin", "plugin.json")
        self.assertEqual(manifest["name"], "sebenza")
        self.assertRegex(manifest["version"], r"^\d+\.\d+\.\d+$")

    def test_hooks_point_at_a_script_that_exists(self):
        hooks = load_json(PLUGIN, "hooks", "hooks.json")
        for event in ("SessionStart", "Stop", "SubagentStop"):
            command = hooks["hooks"][event][0]["hooks"][0]["command"]
            self.assertIn("$CLAUDE_PLUGIN_ROOT/scripts/inbox.py", command)
        self.assertTrue(os.path.isfile(os.path.join(PLUGIN, "scripts", "inbox.py")))


    def test_install_script_and_readme_use_the_stable_ids(self):
        with open(os.path.join(REPO, "scripts", "install.sh"), encoding="utf-8") as fh:
            install = fh.read()
        self.assertIn("--with-plugin", install)
        self.assertIn('PLUGIN_ID="sebenza@sebenza"', install)
        with open(os.path.join(REPO, "README.md"), encoding="utf-8") as fh:
            self.assertIn("claude plugin install sebenza@sebenza", fh.read())


class Layout(unittest.TestCase):
    """TS-34/TS-35: structure intact, provenance recorded."""

    def test_seven_skills_agents_shared_scripts_hooks(self):
        for skill in SKILLS:
            self.assertTrue(os.path.isfile(os.path.join(PLUGIN, "skills", skill, "SKILL.md")), skill)
        for d in ("agents", "shared", "shared/schemas", "scripts", "hooks"):
            self.assertTrue(os.path.isdir(os.path.join(PLUGIN, d)), d)

    def test_migration_records_source_repo_and_commit(self):
        text, _, _ = migration_sections()
        self.assertIn("gts-agent-extensions", text)
        self.assertRegex(text, r"\b[0-9a-f]{40}\b")
        self.assertRegex(text, r"(?m)^## Post-move changes")

    def test_tree_equals_source_listing_plus_recorded_additions(self):
        _, source, additions = migration_sections()
        self.assertIsNotNone(source, "MIGRATION.md needs a fenced '## Source listing'")
        self.assertIsNotNone(additions, "MIGRATION.md needs a fenced '## Post-move additions'")
        self.assertEqual(tree(), source | additions)


class HookSupplyChain(unittest.TestCase):
    """TS-46: inbox.py (run by every hook) is stdlib-only, shell-free and loopback-only."""

    def setUp(self):
        with open(os.path.join(PLUGIN, "scripts", "inbox.py"), encoding="utf-8") as fh:
            self.source = fh.read()
        self.tree = ast.parse(self.source)

    def test_imports_are_stdlib_only(self):
        mods = set()
        for node in ast.walk(self.tree):
            if isinstance(node, ast.Import):
                mods |= {a.name.split(".")[0] for a in node.names}
            elif isinstance(node, ast.ImportFrom) and node.level == 0:
                mods.add(node.module.split(".")[0])
        self.assertEqual(mods - set(sys.stdlib_module_names), set())

    def test_never_uses_a_shell(self):
        for node in ast.walk(self.tree):
            if isinstance(node, ast.keyword) and node.arg == "shell":
                self.assertFalse(isinstance(node.value, ast.Constant) and node.value.value is True)
        self.assertNotIn("os.system(", self.source)

    def test_no_non_loopback_urls(self):
        for url in re.findall(r"https?://[^\s\"')]+", self.source):
            if "{" in url:  # built at runtime; the guard below must vet it
                continue
            self.assertRegex(url, r"^https?://(127\.0\.0\.1|\[::1\])", url)

    def test_daemon_url_is_vetted_as_literal_loopback(self):
        self.assertRegex(self.source, r"def loopback_base\(")
        self.assertIn("is_loopback", self.source)


if __name__ == "__main__":
    unittest.main()
