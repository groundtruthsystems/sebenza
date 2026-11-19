"""Print the default review rules an existing project lacks, as a unified diff.

The standards review enforces only rules tagged in the documents `.ai/sebenza/index.md`
lists. This compares the project with this skill's template guide and prints, for the
operator to approve, the rules for each category the project has no rule for, plus the
index line that makes the guide a standards document. It writes nothing; apply the diff
with `git apply` only after the operator approves it.

usage: propose_rules.py [--root <project root>]
exit:  0 diff printed, or nothing to propose; 2 no `.ai/sebenza/` workspace
"""

import argparse
import difflib
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TEMPLATE = os.path.join(os.path.dirname(HERE), "assets", "code_styleguides", "general.md")
GUIDE = ".ai/sebenza/code_styleguides/general.md"
INDEX = ".ai/sebenza/index.md"
INDEX_LINE = "- [General Code Style Guide](./code_styleguides/general.md)\n"
SECTION = "## Review Rules"
RULE_TAG = re.compile(
    r"^\s*(?:[-*+] +|\d+[.)] +)?\[rule:([a-z0-9][a-z0-9-]{0,63}) category=([a-z0-9][a-z0-9-]{0,63})"
)
LINK = re.compile(r"\]\(([^)\s#]+)")


def read(path):
    try:
        with open(path, encoding="utf-8") as fh:
            return fh.read()
    except OSError:
        return None


def tagged(text):
    """(rule id, category, line) of each rule tag outside fenced blocks."""
    out, fenced = [], False
    for line in (text or "").splitlines(keepends=True):
        if line.lstrip().startswith(("```", "~~~")):
            fenced = not fenced
        elif not fenced and RULE_TAG.match(line):
            m = RULE_TAG.match(line)
            out.append((m.group(1), m.group(2), line))
    return out


def listed_docs(root, index_md):
    """The workspace paths the index links, a directory as its `.md` files."""
    paths = []
    for target in LINK.findall(index_md or ""):
        path = os.path.normpath(os.path.join(root, ".ai", "sebenza", target))
        if os.path.isdir(path):
            paths += [os.path.join(path, n) for n in sorted(os.listdir(path)) if n.endswith(".md")]
        else:
            paths.append(path)
    return paths


def index_covers_guide(root, index_md):
    guide = os.path.normpath(os.path.join(root, GUIDE))
    return guide in listed_docs(root, index_md)


def missing_rules(root, index_md, template_rules):
    docs = set(listed_docs(root, index_md)) | {os.path.normpath(os.path.join(root, GUIDE))}
    have = [r for d in sorted(docs) for r in tagged(read(d))]
    ids, cats = {r[0] for r in have}, {r[1] for r in have}
    return [r for r in template_rules if r[1] not in cats and r[0] not in ids]


def section_intro(template):
    """The template's Review Rules heading and prose, up to its first rule."""
    start = template.index(SECTION)
    first = template.index("[rule:", start)
    return template[start:template.rindex("\n", start, first) + 1]


def proposed_guide(guide, template, missing):
    if guide is None:
        return template
    text = guide if guide.endswith("\n") else guide + "\n"
    if SECTION not in text:
        text += "\n" + section_intro(template)
    return text + "".join(line for _, _, line in missing)


def proposed_index(index_md):
    lines = (index_md or "").splitlines(keepends=True)
    if lines and not lines[-1].endswith("\n"):
        lines[-1] += "\n"
    anchors = [i for i, l in enumerate(lines) if "](./code_styleguides" in l]
    anchors = anchors or [i for i, l in enumerate(lines) if l.strip() == "## Workflow"]
    at = anchors[-1] + 1 if anchors else len(lines)
    return "".join(lines[:at] + [INDEX_LINE] + lines[at:])


def diff(path, old, new):
    old_lines = (old or "").splitlines(keepends=True)
    new_lines = new.splitlines(keepends=True)
    before = "a/" + path if old is not None else "/dev/null"
    return "".join(difflib.unified_diff(old_lines, new_lines, before, "b/" + path))


def proposal(root):
    template = read(TEMPLATE)
    index_md, guide = read(os.path.join(root, INDEX)), read(os.path.join(root, GUIDE))
    missing = missing_rules(root, index_md, tagged(template))
    if not missing:
        return ""
    out = diff(GUIDE, guide, proposed_guide(guide, template, missing))
    if not index_covers_guide(root, index_md):
        out += diff(INDEX, index_md, proposed_index(index_md))
    return out


def project_root(given):
    if given:
        return os.path.abspath(given)
    git = subprocess.run(["git", "rev-parse", "--show-toplevel"],
                         capture_output=True, text=True, check=False)
    return git.stdout.strip() if git.returncode == 0 else os.getcwd()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", help="the project root (default: the git top level)")
    root = project_root(parser.parse_args(argv).root)
    if not os.path.isdir(os.path.join(root, ".ai", "sebenza")):
        print(f"no .ai/sebenza/ workspace under {root}; run setup first", file=sys.stderr)
        return 2
    out = proposal(root)
    if not out:
        print("The default review rules are already adopted; nothing to propose.", file=sys.stderr)
    sys.stdout.write(out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
