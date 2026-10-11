#!/usr/bin/env python3
"""Select affected PR checks and require their successful completion."""

import json
import os
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
LANES = {"backend", "desktop", "mobile", "android", "services"}
NATIVE = LANES - {"services"}
DOCS = {
    "AGENTS.md", "ARCHITECTURE.md", "CHANGELOG.md", "CONTRIBUTING.md",
    "README.md", "README.zh-CN.md", "LICENSE", "NOTICE",
    "services/pairing-relay/README.md", "tests/fixtures/README.md",
}
# Node is also exercised by Desktop and Mobile's real-Node contract fixtures.
DEPENDENTS = (
    (("apps/desktop/", "crates/content/"), {"desktop"}),
    (("apps/host/",), {"backend"}),
    (("crates/node-runtime/", "crates/embedding/", "crates/ocr/"),
     {"backend", "desktop", "mobile"}),
    (("crates/protocol/", "crates/link/", "crates/client/",
      "crates/speech/", "crates/model-assets/", "sdk/", "schemas/", "plugins/"), NATIVE),
    (("apps/mobile/", "crates/mobile-bridge/", "vendor/flutter-markdown-plus/"),
     {"mobile", "android"}),
    (("tests/mobile-contract/",), {"mobile"}),
    (("services/pairing-relay/",), {"services"}),
)


def select_paths(paths):
    selected = set()
    for path in paths:
        if (path in DOCS or path.startswith("third_party_licenses/")
                or (path.startswith("sdk/") and path.endswith(".md"))):
            continue
        if path == "plugins":  # A changed submodule pointer includes its SDK and assets.
            selected.update(NATIVE)
            continue
        if path == "themes" or path.startswith("themes/"):
            # Theme sources are imported explicitly, never embedded in binaries.
            continue
        for prefixes, lanes in DEPENDENTS:
            if path.startswith(prefixes):
                selected.update(lanes)
                break
        else:
            # Includes shared configuration, dependencies, toolchains and new owners.
            return set(LANES)
    return selected


def changed_paths(root, base, head):
    for revision in (base, head):
        if not isinstance(revision, str) or not re.fullmatch(r"[0-9a-fA-F]{40}|[0-9a-fA-F]{64}", revision):
            raise ValueError("Invalid PR revision")
    # Compare from the merge base, including both sides of a rename and deletions.
    result = subprocess.run(
        ["git", "diff", "--name-only", "-z", "--no-renames", f"{base}...{head}", "--"],
        cwd=root, check=True, capture_output=True,
    )
    return [os.fsdecode(path) for path in result.stdout.split(b"\0") if path]


def select_event(event, event_path, root=ROOT):
    if event != "pull_request":
        return set(LANES)
    try:
        payload = json.loads(Path(event_path).read_text(encoding="utf-8"))
        request = payload["pull_request"]
        return select_paths(changed_paths(root, request["base"]["sha"], request["head"]["sha"]))
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError):
        print("PR changes unavailable; running all checks", file=sys.stderr)
        return set(LANES)


def outputs(selected):
    kinds = [kind for kind in ("backend", "desktop") if kind in selected]
    return {
        **{lane: str(lane in selected).lower() for lane in sorted(LANES)},
        "rust": str(bool(kinds)).lower(),
        # Job-level gating skips Rust when empty; GitHub still needs a valid matrix.
        "rust_matrix": json.dumps({"kind": kinds or ["backend"]}, separators=(",", ":")),
    }


def require_results(results):
    jobs = ("source", "rust", "mobile", "android")
    if not isinstance(results, dict) or any(not isinstance(results.get(job), dict) for job in jobs):
        raise ValueError("Missing check results")
    source = results["source"]
    if source.get("result") != "success":
        raise ValueError("Source checks did not pass")
    selection = source.get("outputs", {})
    if not isinstance(selection, dict) or any(selection.get(lane) not in ("true", "false") for lane in LANES):
        raise ValueError("Missing check selection")
    selected = {lane for lane in LANES if selection[lane] == "true"}
    expected = outputs(selected)
    if selection.get("rust") != expected["rust"]:
        raise ValueError("Inconsistent Rust selection")
    try:
        matrix = json.loads(selection["rust_matrix"])
    except (ValueError, TypeError, KeyError) as error:
        raise ValueError("Missing Rust matrix") from error
    if matrix != json.loads(expected["rust_matrix"]):
        raise ValueError("Inconsistent Rust matrix")
    for name in ("services", *jobs[1:]):
        result = selection.get("services_result") if name == "services" else results[name].get("result")
        required = expected[name] == "true"
        if result != "success" and (required or result != "skipped"):
            raise ValueError(f"{name}: required check did not pass ({result})")


def main():
    if sys.argv[1:] == ["select"]:
        selection = outputs(select_event(os.getenv("GITHUB_EVENT_NAME"), os.getenv("GITHUB_EVENT_PATH")))
        with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as target:
            target.writelines(f"{key}={value}\n" for key, value in selection.items())
        print("Selected checks: " + ", ".join(lane for lane in sorted(LANES) if selection[lane] == "true"))
    elif sys.argv[1:] == ["report"]:
        try:
            results = json.loads(os.environ["CHECK_RESULTS"])
            require_results(results)
        except (ValueError, KeyError) as error:
            raise SystemExit(str(error)) from error
        for name, job in results.items():
            print(f"{name}: {job['result']}")
        print(f"services: {results['source']['outputs']['services_result']}")
        print("Checks passed")
    else:
        raise SystemExit("Usage: ci.py select|report")


if __name__ == "__main__":
    main()
