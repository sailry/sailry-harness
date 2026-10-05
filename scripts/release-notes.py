#!/usr/bin/env python3
"""Generate release notes and a cumulative changelog from an immutable version tag."""

import argparse
from datetime import datetime, timezone
from pathlib import Path
import re
import subprocess
from urllib.parse import quote

TAG = re.compile(r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?")


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def escape(text):
    return re.sub(r"([\\`*_\[\]<>])", r"\\\1", text)


def generate(root, tag, repository, date):
    if not TAG.fullmatch(tag):
        raise ValueError("Expected a version tag such as v0.1.0")
    revision = git(root, "rev-parse", f"refs/tags/{tag}^{{commit}}")
    parents = git(root, "rev-list", "--parents", "-n", "1", revision).split()[1:]
    previous = None
    if parents:
        result = subprocess.run(["git", "-C", str(root), "describe", "--tags", "--abbrev=0",
                                 "--match", "v[0-9]*", parents[0]], capture_output=True, text=True)
        if result.returncode == 0 and TAG.fullmatch(result.stdout.strip()):
            previous = result.stdout.strip()
    span = f"{previous}..{revision}" if previous else revision
    commits = git(root, "log", "--no-merges", "--reverse", "--format=%H%x09%s", span)
    lines = [f"## [{tag[1:]}]({repository}/releases/tag/{quote(tag)}) — {date}", ""]
    for line in commits.splitlines():
        commit, subject = line.split("\t", 1)
        # The previous release's generated documentation is not a product change.
        if subject.startswith("docs: update changelog for v"):
            continue
        lines.append(f"- {escape(subject)} ([{commit[:7]}]({repository}/commit/{commit}))")
    if len(lines) == 2:
        lines.append("- No product changes")
    if previous:
        lines.extend(["", f"[Full changelog]({repository}/compare/{quote(previous)}...{quote(tag)})"])
    return "\n".join(lines) + "\n"


def update(existing, entry, tag):
    if re.search(rf"^## \[{re.escape(tag[1:])}\]", existing, re.MULTILINE):
        return existing
    match = re.search(r"^## ", existing, re.MULTILINE)
    prefix = existing[:match.start()] if match else existing
    history = existing[match.start():] if match else ""
    return prefix.rstrip() + "\n\n" + entry.rstrip() + "\n" + ("\n" + history if history else "")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repository", required=True, help="Public repository URL")
    parser.add_argument("--notes", type=Path, required=True)
    parser.add_argument("--changelog", type=Path, required=True)
    args = parser.parse_args()
    entry = generate(args.root, args.tag, args.repository.rstrip("/"), datetime.now(timezone.utc).date().isoformat())
    args.notes.write_text(entry, encoding="utf-8")
    args.changelog.write_text(update(args.changelog.read_text(encoding="utf-8"), entry, args.tag), encoding="utf-8")


if __name__ == "__main__":
    main()
