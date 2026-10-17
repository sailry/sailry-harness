#!/usr/bin/env python3
"""Inspect local validation inputs without changing a checkout or running checks."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess

import ci


def git(root, *arguments):
    return subprocess.run(
        ["git", "--no-optional-locks", *arguments], cwd=root,
        check=True, capture_output=True,
    ).stdout


def revision(root, value):
    resolved = git(root, "rev-parse", "--verify", "--end-of-options",
                   f"{value}^{{commit}}").decode().strip()
    if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", resolved):
        raise ValueError("Git did not resolve a commit")
    return resolved


def paths(value):
    return {os.fsdecode(path) for path in value.split(b"\0") if path}


def worktree_paths(root):
    common = ["--name-only", "--no-renames", "--ignore-submodules=none", "-z"]
    # Keep both differences: an unstaged edit can undo a staged edit in HEAD's view.
    return (paths(git(root, "diff", "--cached", *common, "--"))
            | paths(git(root, "diff", *common, "--"))
            | paths(git(root, "ls-files", "--others", "--exclude-standard", "-z")))


def gitlinks(root, prefix=""):
    committed = {}
    for item in git(root, "ls-tree", "-r", "-z", "HEAD").split(b"\0"):
        if item:
            metadata, path = item.split(b"\t", 1)
            mode, _, commit = metadata.split()
            if mode == b"160000":
                committed[os.fsdecode(path)] = commit.decode()
    indexed = {}
    for item in git(root, "ls-files", "--stage", "-z").split(b"\0"):
        if item:
            metadata, path = item.split(b"\t", 1)
            mode, commit, stage = metadata.split()
            if mode == b"160000" and stage == b"0":
                indexed[os.fsdecode(path)] = commit.decode()
    result = []
    for path in sorted(committed.keys() | indexed.keys()):
        directory = root / path
        initialized = not directory.is_symlink() and (directory / ".git").exists()
        actual, dirty, children = None, False, []
        if initialized:
            actual = revision(directory, "HEAD")
            dirty = bool(worktree_paths(directory))
            children = gitlinks(directory, prefix + path + "/")
        result.append({
            "path": prefix + path, "expected": committed.get(path),
            "index": indexed.get(path), "actual": actual,
            "initialized": initialized, "dirty": dirty,
        })
        result.extend(children)
    return result


def inspect_source(root, mode, base=None, head=None, full=False, event="push"):
    root = Path(root).resolve()
    if mode not in {"worktree", "candidate"}:
        raise ValueError("Expected worktree or candidate source mode")
    if event not in {"push", "pull_request"}:
        raise ValueError("Expected push or pull_request comparison")
    if mode == "candidate" and head is None:
        raise ValueError("Candidate validation requires an explicit head")
    actual = revision(root, "HEAD")
    candidate = revision(root, head if head is not None else "HEAD")
    if candidate != actual:
        raise ValueError("The checkout must match the candidate head; use an existing clean worktree")
    selected_paths = worktree_paths(root)
    links = gitlinks(root)
    dirty = bool(selected_paths)
    if mode == "worktree" and base is None:
        base = "HEAD"
    base = revision(root, base) if base is not None else None
    if mode == "candidate":
        if dirty:
            raise ValueError("Candidate validation requires a clean checkout, including submodules")
        if any(not item["initialized"] or item["dirty"]
               or item["expected"] != item["index"] or item["index"] != item["actual"]
               for item in links):
            raise ValueError("Candidate submodules must be initialized, clean and match their gitlinks")
        if base is None and not full:
            raise ValueError("Candidate selection requires an explicit base or --all")
        if base == actual and not full:
            raise ValueError("Base and head are identical; choose an earlier base or --all")
    if base is not None:
        if event == "push":
            git(root, "merge-base", "--is-ancestor", base, actual)
        selected_paths.update(ci.changed_paths(root, base, actual, merge_base=event == "pull_request"))
    selected = set(ci.LANES) if full else ci.select_paths(selected_paths)
    return {
        "mode": mode, "head": actual,
        "head_tree": git(root, "rev-parse", "HEAD^{tree}").decode().strip(),
        "base": base, "event": event, "scope": "all" if full else "affected",
        "dirty": dirty, "paths": sorted(selected_paths), "gitlinks": links,
        "selection": ci.outputs(selected), "lint_packages": ci.lint_packages(selected),
        "inspection_only": True,
        "empty_reason": "No affected checks selected; no validation was performed" if not selected else None,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("worktree", "candidate"))
    parser.add_argument("--base")
    parser.add_argument("--head")
    parser.add_argument("--event", choices=("push", "pull_request"), default="push")
    parser.add_argument("--all", action="store_true", dest="full")
    arguments = parser.parse_args()
    try:
        report = inspect_source(ci.ROOT, arguments.mode, arguments.base,
                                arguments.head, arguments.full, arguments.event)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.error(str(error))
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
