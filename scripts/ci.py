#!/usr/bin/env python3
"""Select affected PR checks and require their successful completion."""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[1]
LANES = {"shared", "desktop", "host", "mobile", "mobile_ffi", "android", "services"}
NATIVE = LANES - {"services"}
PRODUCTS = {
    "desktop": {"shared", "desktop"},
    "host": {"shared", "host"},
    "mobile": {"shared", "mobile", "mobile_ffi", "android"},
}
FIXTURES = {
    "shared": {
        "test_ci_resources", "test_ci_selection", "test_locales", "test_macos_package",
        "test_names", "test_onnx_runtime", "test_public_source", "test_publish_packages",
        "test_python_launcher", "test_python_runtime", "test_release_notes",
        "test_release_versions", "test_suites",
    },
    "desktop": {
        "test_app_icons", "test_desktop_launch", "test_publish_update_feed",
        "test_theme_audit", "test_ui_tasks",
    },
    "host": {
        "test_acp_wrapper", "test_host_install", "test_host_service", "test_host_tasks",
        "test_linux_sysroot",
    },
    "mobile": {"test_android_package"},
}
DOCS = {
    "AGENTS.md", "ARCHITECTURE.md", "CHANGELOG.md", "CONTRIBUTING.md",
    "README.md", "README.zh-CN.md", "LICENSE", "NOTICE",
    "services/pairing-relay/README.md", "tests/fixtures/README.md",
}
# Node is also exercised by Desktop and Mobile's real-Node contract fixtures.
DEPENDENTS = (
    (("tools/release/",), {"shared"}),
    (("apps/desktop/", "crates/content/"), {"desktop"}),
    (("apps/host/",), {"host"}),
    (("crates/node-runtime/", "crates/embedding/", "crates/ocr/"),
     {"shared", "desktop", "host", "mobile", "mobile_ffi"}),
    (("crates/protocol/", "crates/link/", "crates/client/",
      "crates/speech/", "crates/model-assets/", "sdk/", "schemas/", "plugins/"), NATIVE),
    (("apps/mobile/", "vendor/flutter-markdown-plus/"), {"mobile", "android"}),
    (("crates/mobile-bridge/",), {"mobile", "mobile_ffi", "android"}),
    (("tests/mobile-contract/",), {"mobile_ffi"}),
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
    return {lane: str(lane in selected).lower() for lane in sorted(LANES)}


def fixture_names(root, owner):
    registered = [name for names in FIXTURES.values() for name in names]
    actual = {path.stem for path in (root / "scripts/tests").glob("test_*.py")}
    if len(registered) != len(set(registered)) or set(registered) != actual:
        raise ValueError("Every script fixture must have exactly one CI owner")
    return sorted(FIXTURES[owner])


def run_fixtures(owner):
    suite = unittest.TestSuite()
    loader = unittest.TestLoader()
    for name in fixture_names(ROOT, owner):
        suite.addTests(loader.discover(str(ROOT / "scripts/tests"), pattern=f"{name}.py"))
    return unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful()


def locked_version(root, name):
    packages = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))["package"]
    versions = [package["version"] for package in packages if package.get("name") == name]
    if len(versions) != 1:
        raise ValueError(f"Expected one locked {name} package")
    return versions[0]


def source_cache_key(lockfile):
    packages = tomllib.loads(lockfile.read_text(encoding="utf-8"))["package"]
    identities = sorted(
        (package["name"], package["version"], package["source"], package.get("checksum", ""))
        for package in packages if "source" in package
    )
    digest = hashlib.sha256(json.dumps(identities, separators=(",", ":")).encode()).hexdigest()
    return f"cargo-sources-v1-{digest}"


def speech_cache_key(root, target, mode):
    if not re.fullmatch(r"[a-z0-9_]+(?:-[a-z0-9_]+)+", target):
        raise ValueError("Invalid speech target")
    if mode not in {"shared", "static"}:
        raise ValueError("Invalid speech link mode")
    sherpa = locked_version(root, "sherpa-onnx")
    system = locked_version(root, "sherpa-onnx-sys")
    workspace = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    dependency = workspace["workspace"]["dependencies"]["sherpa-onnx"]
    if not isinstance(dependency, dict) or dependency.get("version") != f"={sherpa}":
        raise ValueError("sherpa-onnx must remain pinned to its locked version")
    return f"speech-native-v1-sherpa-{sherpa}-sys-{system}-{target}-{mode}"


def require_results(results, product=None):
    required = PRODUCTS[product] if product else LANES
    jobs = ("source", *sorted(required))
    if not isinstance(results, dict) or any(not isinstance(results.get(job), dict) for job in jobs):
        raise ValueError("Missing check results")
    source = results["source"]
    if source.get("result") != "success":
        raise ValueError("Source checks did not pass")
    selection = source.get("outputs", {})
    if not isinstance(selection, dict) or any(selection.get(lane) not in ("true", "false") for lane in LANES):
        raise ValueError("Missing check selection")
    selected = {lane for lane in LANES if selection[lane] == "true"}
    for name in jobs[1:]:
        result = results[name].get("result")
        if product and (name not in selected or result != "success"):
            raise ValueError(f"{name}: product prerequisite did not pass ({result})")
        if result != "success" and (name in selected or result != "skipped"):
            raise ValueError(f"{name}: required check did not pass ({result})")


def cache_sources(mode, workspace, home):
    archive = workspace / ".cargo-sources"
    source, destination = (home, archive) if mode == "save" else (archive, home)
    for relative in ("registry/index", "registry/cache", "git/db"):
        directory = source / relative
        if directory.is_dir():
            shutil.copytree(directory, destination / relative, dirs_exist_ok=True)


def cache_usage():
    repository = os.environ["GH_REPO"]
    result = subprocess.run(
        ["gh", "api", f"repos/{repository}/actions/cache/usage"],
        check=True, text=True, capture_output=True,
    )
    usage = json.loads(result.stdout)
    size = usage["active_caches_size_in_bytes"]
    count = usage["active_caches_count"]
    message = f"Repository cache: {size / 2**30:.3f} GiB / 10 GiB budget ({count} entries)"
    print(message)
    if size >= 9 * 2**30:
        print("::warning::Cache budget nearly full; GitHub LRU may evict compiler objects")
    if os.getenv("GITHUB_STEP_SUMMARY"):
        with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a", encoding="utf-8") as output:
            output.write(message + "\n\n")


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "source-cache" and sys.argv[2] in {"save", "restore"}:
        cache_sources(sys.argv[2], Path(os.environ["GITHUB_WORKSPACE"]),
                      Path(os.getenv("CARGO_HOME", str(Path.home() / ".cargo"))))
    elif len(sys.argv) == 3 and sys.argv[1] == "source-key":
        key = source_cache_key(Path(sys.argv[2]))
        with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as target:
            target.write(f"key={key}\n")
        print(key)
    elif sys.argv[1:] == ["cache-usage"]:
        cache_usage()
    elif sys.argv[1:] == ["select"]:
        selection = outputs(select_event(os.getenv("GITHUB_EVENT_NAME"), os.getenv("GITHUB_EVENT_PATH")))
        with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as target:
            target.writelines(f"{key}={value}\n" for key, value in selection.items())
        print("Selected checks: " + ", ".join(lane for lane in sorted(LANES) if selection[lane] == "true"))
    elif len(sys.argv) == 4 and sys.argv[1] == "speech-key":
        try:
            key = speech_cache_key(ROOT, sys.argv[2], sys.argv[3])
            with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as target:
                target.write(f"key={key}\n")
        except (OSError, ValueError, KeyError, TypeError) as error:
            raise SystemExit(str(error)) from error
        print(key)
    elif len(sys.argv) == 3 and sys.argv[1] == "fixtures" and sys.argv[2] in FIXTURES:
        raise SystemExit(0 if run_fixtures(sys.argv[2]) else 1)
    elif sys.argv[1:] == ["report"] or (
        len(sys.argv) == 3 and sys.argv[1] == "report" and sys.argv[2] in PRODUCTS
    ):
        try:
            results = json.loads(os.environ["CHECK_RESULTS"])
            require_results(results, sys.argv[2] if len(sys.argv) == 3 else None)
        except (ValueError, KeyError) as error:
            raise SystemExit(str(error)) from error
        for name, job in results.items():
            print(f"{name}: {job['result']}")
        print(f"{sys.argv[2].title()} ready" if len(sys.argv) == 3 else "Checks passed")
    else:
        raise SystemExit(
            "Usage: ci.py source-key <lockfile>|source-cache <save|restore>|cache-usage|select|report [desktop|host|mobile]|fixtures <owner>|"
            "speech-key <target> <shared|static>"
        )


if __name__ == "__main__":
    main()
