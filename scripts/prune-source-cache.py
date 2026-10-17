#!/usr/bin/env python3
"""Remove legacy source archives only after their canonical replacement exists."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
from urllib.parse import urlencode

import ci


MAIN_REF = "refs/heads/main"
ACTIVE_STATES = ("in_progress", "queued", "requested", "waiting", "pending")


def legacy_key(lockfile):
    # hashFiles hashes each file before hashing the concatenated file digests.
    digest = hashlib.sha256(lockfile.read_bytes()).digest()
    return "cargo-sources-v1-" + hashlib.sha256(digest).hexdigest()


def source_keys(saved_key, lockfiles):
    if not re.fullmatch(r"cargo-sources-v1-[0-9a-f]{64}", saved_key):
        raise ValueError("Invalid canonical source cache key")
    keys = set()
    for lockfile in lockfiles:
        if ci.source_cache_key(lockfile) != saved_key:
            raise ValueError("The legacy lockfile has different external dependencies")
        keys.add(legacy_key(lockfile))
    return keys - {saved_key}


def api(repository, endpoint, method="GET"):
    result = subprocess.run(
        ["gh", "api", "--method", method, f"repos/{repository}/{endpoint}"],
        capture_output=True, text=True, timeout=30,
    )
    if result.returncode:
        raise RuntimeError("The GitHub cache maintenance request failed")
    return json.loads(result.stdout) if method == "GET" else None


def records(repository, endpoint, field, parameters):
    page = 1
    while True:
        query = urlencode({**parameters, "per_page": 100, "page": page})
        payload = api(repository, f"{endpoint}?{query}")
        entries = payload[field]
        if not isinstance(entries, list):
            raise ValueError("Invalid GitHub cache maintenance response")
        yield from entries
        if len(entries) < 100:
            return
        page += 1


def cache_records(repository, key):
    return [entry for entry in records(
        repository, "actions/caches", "actions_caches", {"key": key, "ref": MAIN_REF},
    ) if entry.get("key") == key and entry.get("ref") == MAIN_REF]


def active_consumers(repository, current_run):
    for status in ACTIVE_STATES:
        for run in records(repository, "actions/runs", "workflow_runs", {"status": status}):
            if not isinstance(run.get("id"), int) or not run.get("status"):
                raise ValueError("Invalid workflow run identity")
            if run["id"] != current_run:
                return True
    return False


def prune(repository, current_run, saved_key, lockfiles):
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*", repository):
        raise ValueError("Invalid repository name")
    if not isinstance(current_run, int) or current_run <= 0:
        raise ValueError("Invalid workflow run identity")
    keys = source_keys(saved_key, lockfiles)
    replacements = cache_records(repository, saved_key)
    if not replacements:
        raise ValueError("The canonical source archive has not been saved")
    versions = set()
    for entry in replacements:
        if (not isinstance(entry.get("id"), int) or entry["id"] <= 0
                or not isinstance(entry.get("size_in_bytes"), int) or entry["size_in_bytes"] <= 0
                or not isinstance(entry.get("version"), str) or not entry["version"]):
            raise ValueError("The canonical source archive could not be verified")
        versions.add(entry["version"])
    # A release or another queued/running job may still restore a legacy key.
    # Future consumers can fall back to the verified replacement in main's scope.
    if active_consumers(repository, current_run):
        print("Source cache cleanup deferred while other workflows are active")
        return []
    obsolete = []
    for key in sorted(keys):
        for entry in cache_records(repository, key):
            if entry.get("version") not in versions:
                continue
            if not isinstance(entry.get("id"), int) or entry["id"] <= 0:
                raise ValueError("Invalid legacy source archive identity")
            obsolete.append(entry)
    for entry in obsolete:
        api(repository, f"actions/caches/{entry['id']}", "DELETE")
        print(f"Removed superseded source archive {entry['id']} ({entry['key']})")
    return [entry["id"] for entry in obsolete]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--saved-key", required=True)
    parser.add_argument("--legacy-lockfile", action="append", type=Path, default=[])
    arguments = parser.parse_args()
    if (os.environ.get("GITHUB_REF") != MAIN_REF
            or os.environ.get("GITHUB_EVENT_NAME") not in {"push", "workflow_dispatch"}):
        print("Source cache cleanup only runs on main pushes or dispatches")
        return 0
    if not os.environ.get("GH_TOKEN"):
        raise ValueError("Source cache cleanup requires an Actions write token")
    root = Path(os.environ["GITHUB_WORKSPACE"])
    prune(os.environ["GITHUB_REPOSITORY"], int(os.environ["GITHUB_RUN_ID"]),
          arguments.saved_key, [root / "Cargo.lock", *arguments.legacy_lockfile])
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (KeyError, ValueError, RuntimeError, OSError, subprocess.SubprocessError) as error:
        print(f"::warning::Source cache cleanup failed: {error}")
        raise SystemExit(1) from None
