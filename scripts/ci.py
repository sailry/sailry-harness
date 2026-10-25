#!/usr/bin/env python3
"""Select affected checks and require their successful completion."""

import argparse
import hashlib
import importlib.util
import json
import os
import platform
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
        "test_ci_local", "test_ci_resources", "test_ci_selection", "test_desktop_adhoc", "test_desktop_build", "test_desktop_recovery", "test_locales", "test_macos_package",
        "test_names", "test_ocu_helper", "test_onnx_runtime", "test_llama_runtime", "test_code_parsers", "test_public_source", "test_publish_packages",
        "test_python_launcher", "test_python_runtime", "test_release_notes", "test_source_cache",
        "test_release_versions", "test_suites",
    },
    "desktop": {
        "test_app_icons", "test_desktop_launch", "test_publish_update_feed",
        "test_theme_audit", "test_ui_tasks",
    },
    "host": {
        "test_acp_wrapper", "test_host_install", "test_host_service", "test_host_tasks", "test_host_update",
        "test_linux_sysroot", "test_host_compatibility",
    },
    "mobile": {"test_android_package"},
}
DOCS = {
    "AGENTS.md", "ARCHITECTURE.md", "CHANGELOG.md", "CONTRIBUTING.md",
    "README.md", "README.zh-CN.md", "LICENSE", "NOTICE", "CLA.md", "sdk/LICENSE",
    "services/pairing-relay/README.md", "tests/fixtures/README.md",
    "COMMERCIAL.md", "CODE_OF_CONDUCT.md", "SECURITY.md",
    "docs/models.md", "docs/hosts.md", "docs/projects.md",
}
LINT_PACKAGES = {
    "shared": (
        "sailry-protocol", "sailry-link", "sailry-client", "sailry-node-runtime",
        "sailry-speech", "sailry-embedding", "sailry-decision", "sailry-model-assets", "sailry-ocr", "sailry-release",
    ),
    "desktop": ("sailry-content", "sailry-desktop"),
    "host": ("sailry-host",),
    "mobile": ("sailry-mobile-bridge",),
}
# Node is also exercised by Desktop and Mobile's real-Node contract fixtures.
DEPENDENTS = (
    (("scripts/package/code-parsers.",), {"shared", "desktop", "host", "mobile_ffi"}),
    (("tools/release/",), {"shared"}),
    (("apps/desktop/", "crates/content/"), {"desktop"}),
    (("apps/host/",), {"host"}),
    (("crates/node-runtime/", "crates/embedding/", "crates/ocr/", "crates/decision/"),
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
    fixtures = {f"scripts/tests/{name}.py": set() if owner == "shared" else {owner}
                for owner, names in FIXTURES.items() for name in names}
    for path in paths:
        if (path in DOCS or path.startswith(("third_party_licenses/", "assets/readme/"))
                or (path.startswith("sdk/") and path.endswith(".md"))):
            continue
        if path in fixtures:
            # Source always runs its fixtures; product fixtures run with their owner.
            selected.update(fixtures[path])
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


def changed_paths(root, base, head, merge_base=True):
    for revision in (base, head):
        if not isinstance(revision, str) or not re.fullmatch(r"[0-9a-fA-F]{40}|[0-9a-fA-F]{64}", revision):
            raise ValueError("Invalid change revision")
    # Compare from the merge base, including both sides of a rename and deletions.
    result = subprocess.run(
        ["git", "diff", "--name-only", "-z", "--no-renames",
         f"{base}...{head}" if merge_base else base,
         *([] if merge_base else [head]), "--"],
        cwd=root, check=True, capture_output=True,
    )
    return [os.fsdecode(path) for path in result.stdout.split(b"\0") if path]


def select_event(event, event_path, root=ROOT):
    if event not in {"pull_request", "push"}:
        return set(LANES)
    try:
        payload = json.loads(Path(event_path).read_text(encoding="utf-8"))
        if event == "pull_request":
            request = payload["pull_request"]
            return select_paths(changed_paths(root, request["base"]["sha"], request["head"]["sha"]))
        base, head = payload["before"], payload["after"]
        if (payload.get("ref") != "refs/heads/main" or payload.get("forced") is not False
                or payload.get("created") is not False or payload.get("deleted") is not False
                or any(not isinstance(value, str) or not re.fullmatch(r"[0-9a-fA-F]{40}|[0-9a-fA-F]{64}", value)
                       or not value.strip("0") for value in (base, head))):
            raise ValueError("Push revisions are unavailable")
        current = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root,
                                 check=True, capture_output=True, text=True).stdout.strip()
        if current != head:
            raise ValueError("Push checkout does not match its source")
        subprocess.run(["git", "merge-base", "--is-ancestor", base, head],
                       cwd=root, check=True, capture_output=True)
        return select_paths(changed_paths(root, base, head, merge_base=False))
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError):
        print("Changes unavailable; running all checks", file=sys.stderr)
        return set(LANES)


def lint_packages(selected):
    return sorted({package for lane in selected for package in LINT_PACKAGES.get(lane, ())})


def lint_command(selection, jobs):
    if (not isinstance(selection, dict) or set(selection) != LANES
            or any(value not in {"true", "false"} for value in selection.values())):
        raise ValueError("Missing lint selection")
    if not re.fullmatch(r"[1-9][0-9]*", jobs):
        raise ValueError("Invalid lint job count")
    packages = lint_packages({lane for lane in LANES if selection[lane] == "true"})
    if not packages:
        raise ValueError("No Rust packages selected")
    command = ["cargo", "clippy", "--locked"]
    for package in packages:
        command.extend(["-p", package])
    command.extend(["--all-targets", "--jobs", jobs, "--", "-D", "warnings"])
    return command


def lint(selection, jobs):
    command = lint_command(selection, jobs)
    environment = parser_environment(ROOT, None, libraries=False)
    return subprocess.run(command, env=environment, check=False).returncode


def check_commands(name, jobs="2", root=ROOT, selection=None, env=None):
    """The command source for CI phases and explicit local validation."""
    if not re.fullmatch(r"[1-9][0-9]*", jobs):
        raise ValueError("Invalid build job count")
    python = sys.executable
    environment = os.environ if env is None else env
    target = Path(environment.get("CARGO_TARGET_DIR", str(root / "target")))
    triple = environment.get("CARGO_BUILD_TARGET")
    if triple:
        target /= Path(triple).stem if triple.endswith(".json") else triple
    runtime_host = target / "debug/examples/plugin-test-host"
    if platform.system() == "Windows":
        runtime_host = runtime_host.with_suffix(".exe")
    packages = [part for package in LINT_PACKAGES["shared"] for part in ("-p", package)]
    backend = ["cargo", "nextest", "run", "--locked", *packages]
    desktop = ["cargo", "nextest", "run", "--locked", "-p", "sailry-content", "-p", "sailry-desktop"]
    host = ["cargo", "nextest", "run", "--locked", "-p", "sailry-host"]
    commands = {
        "source": [([python, "scripts/ci.py", "fixtures", "shared"], ".")],
        "names": [([python, "scripts/check-names.py"], ".")],
        "workflows": [([os.getenv("ACTIONLINT", "actionlint"), "-shellcheck=", "-pyflakes="], ".")],
        "format": [(["cargo", "fmt", "--all", "--", "--check"], ".")],
        "backend": [
            (backend + ["--timings", "--build-jobs", jobs, "--profile", "ci", "--test-threads=4"], "."),
            (["cargo", "build", "--locked", "-p", "sailry-node-runtime", "--features", "test-support", "--example", "plugin-test-host", "--profile", "test", "--jobs", jobs], "."),
            ([python, "scripts/test-plugins.py", "--runtime-host", str(runtime_host)], "."),
        ],
        "databases": [(backend + ["--build-jobs", jobs, "--profile", "databases", "--run-ignored", "only", "--filterset", "binary(databases)", "--test-threads=1"], ".")],
        "desktop": [(desktop + ["--lib", "--bin", "sailry-desktop", "--timings", "--build-jobs", jobs, "--profile", "ci"], ".")],
        "desktop-production": [(["cargo", "check", "--locked", "-p", "sailry-desktop", "--jobs", jobs], ".")],
        "desktop-lifecycle": [(["cargo", "nextest", "run", "--locked", "-p", "sailry-desktop", "--test", "lifecycle", "--build-jobs", jobs, "--profile", "lifecycle"], ".")],
        "host": [(host + ["--timings", "--build-jobs", jobs, "--profile", "ci", "--test-threads=4"], ".")],
        "host-mac": [(host + ["--test", "lifecycle", "--timings", "--build-jobs", jobs, "--profile", "ci", "--test-threads=4"], ".")],
        "host-production": [(["cargo", "check", "--locked", "-p", "sailry-host", "--jobs", jobs], ".")],
        "desktop-source": [
            ([python, "scripts/ci.py", "fixtures", "desktop"], "."),
            ([python, "-m", "unittest", "discover", "-s", "apps/desktop/src/content/files/preview/web", "-v"], "."),
            ([python, "themes/scripts/catalog.py", "--check"], "."),
            ([python, "-m", "unittest", "discover", "-s", "themes/scripts/tests", "-v"], "."),
            (["node", "--test", "crates/content/src/markdown/visuals/render.test.mjs"], "."),
            ([python, "scripts/check-locales.py", "--product", "desktop"], "."),
        ],
        "host-source": [([python, "scripts/ci.py", "fixtures", "host"], ".")],
        "mobile-source": [([python, "scripts/ci.py", "fixtures", "mobile"], "."), ([python, "scripts/check-locales.py", "--product", "mobile"], ".")],
        "mobile-format": [(["dart", "format", "--output=none", "--set-exit-if-changed", "lib", "preview", "test", "tool"], "apps/mobile")],
        "mobile-check": [(["flutter", "analyze", "--no-pub"], "apps/mobile"), (["flutter", "test", "--no-pub"], "apps/mobile")],
        "services": [(["pnpm", task], "services/pairing-relay") for task in ("typecheck", "test", "build", "build:edgeone")],
    }
    if name == "lint":
        return [{"argv": lint_command(selection, jobs), "cwd": "."}]
    if name == "scripts":
        listed = subprocess.run(
            ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "*.sh"],
            cwd=root, check=True, capture_output=True,
        ).stdout.split(b"\0")
        paths = sorted({os.fsdecode(path) for path in listed if path and (root / os.fsdecode(path)).is_file()})
        return [{"argv": ["bash", "-n", path], "cwd": "."} for path in paths] + [
            {"argv": [python, "-m", "compileall", "-q", "scripts"], "cwd": "."},
        ]
    if name not in commands:
        raise ValueError(f"Unknown check: {name}")
    return [{"argv": argv, "cwd": cwd} for argv, cwd in commands[name]]


def parser_environment(root, env, libraries):
    spec = importlib.util.spec_from_file_location("code_parsers", root / "scripts/package/code-parsers.py")
    parsers = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(parsers)
    environment = dict(os.environ if env is None else env)
    cache = (root / ".runtime/code-index/resources").absolute()
    target = output = None
    if libraries:
        target = environment.get("CARGO_BUILD_TARGET")
        if target is None:
            toolchain = subprocess.check_output(["rustc", "-vV"], env=environment, text=True)
            target = next(line.removeprefix("host: ") for line in toolchain.splitlines() if line.startswith("host: "))
        build = Path(environment.get("CARGO_TARGET_DIR", str(root / "target")))
        if not build.is_absolute():
            build = root / build
        if environment.get("CARGO_BUILD_TARGET"):
            build /= target
        output = build.absolute() / "debug/code-parsers"
    environment["TSLP_SOURCE_BUNDLE_URL"] = parsers.prepare(target, output, cache, source_only=not libraries)
    # Both settings override the locked dynamic-loading configuration: OFFLINE
    # omits embedded queries, while LANGUAGES requests native grammar compilation.
    environment.pop("TSLP_OFFLINE", None)
    environment.pop("TSLP_LANGUAGES", None)
    if libraries and "apple" in target:
        parsers.sign(target, output, "-")
    return environment


def run_check(name, jobs="2", root=ROOT, selection=None, env=None):
    commands = check_commands(name, jobs, root, selection, env)
    if name in {"backend", "databases", "desktop", "desktop-lifecycle", "host", "host-mac"}:
        env = parser_environment(root, env, libraries=True)
    elif name in {"lint", "desktop-production", "host-production"}:
        env = parser_environment(root, env, libraries=False)
    for command in commands:
        print(f"Check {name}: " + " ".join(command["argv"]), flush=True)
        code = subprocess.run(command["argv"], cwd=root / command["cwd"], env=env, check=False).returncode
        if code:
            return code if code > 0 else 128 - code
    return 0


def local_checks(selected, tier, system):
    checks = ["source", "names", "scripts", "workflows"]
    remaining = []
    if lint_packages(selected):
        checks.append("format")
    for lane in ("desktop", "host", "mobile"):
        if lane in selected:
            checks.append(f"{lane}-source")
    if "mobile" in selected:
        checks.append("mobile-format")
    if tier == "fast" and selected:
        remaining.append("Affected product tests and strict Rust lint were not requested by the fast tier")
    if tier == "affected":
        if lint_packages(selected):
            checks.append("lint")
        if "shared" in selected:
            checks.append("backend")
            remaining.append("Shared database fixtures require configured PostgreSQL and MySQL services")
        if "desktop" in selected:
            if system == "Darwin":
                checks.extend(("desktop", "desktop-production", "desktop-lifecycle"))
            else:
                remaining.append("Desktop CI uses macOS and its pinned Xcode SDK")
        if "host" in selected:
            if system in {"Darwin", "Linux"}:
                checks.extend(("host-mac" if system == "Darwin" else "host", "host-production"))
                remaining.append("Host CI also validates the other Linux/macOS runner")
            else:
                remaining.append("Host CI requires Linux or macOS")
        if "mobile" in selected:
            checks.append("mobile-check")
            remaining.append("Mobile CI also builds unsigned iOS")
        if "services" in selected:
            checks.append("services")
    if "mobile_ffi" in selected:
        remaining.append("Dart FFI contracts require the native bridge and database services")
    if "android" in selected:
        remaining.append("Android CI requires its pinned SDK, NDK and native boundary checks")
    return {"checks": checks, "not_run": remaining}


def local(arguments):
    import ci_local

    parser = argparse.ArgumentParser(description="Validate development changes or an explicitly checked-out candidate")
    parser.add_argument("tier", nargs="?", choices=("fast", "affected"), default="fast")
    parser.add_argument("--base")
    parser.add_argument("--head")
    parser.add_argument("--candidate", action="store_true")
    parser.add_argument("--event", choices=("push", "pull_request"), default="push")
    parser.add_argument("--all", action="store_true", dest="full")
    parser.add_argument("--jobs", default=os.getenv("BUILD_JOBS", os.getenv("CARGO_BUILD_JOBS", "2")))
    parser.add_argument("--target-dir", type=Path)
    parser.add_argument("--plan", action="store_true")
    args = parser.parse_args(arguments)
    if not args.candidate and args.head is not None:
        parser.error("--head requires --candidate")
    target = args.target_dir.resolve() if args.target_dir else Path(os.getenv("CARGO_TARGET_DIR", str(ROOT / "target"))).resolve()
    env = {**os.environ, "CARGO_TARGET_DIR": str(target)}
    try:
        source = ci_local.inspect_source(ROOT, "candidate" if args.candidate else "worktree", args.base, args.head, args.full, args.event)
        selected = {name for name, value in source["selection"].items() if value == "true"}
        plan = local_checks(selected, args.tier, platform.system())
        commands = [{"check": name, **command} for name in plan["checks"]
                    for command in check_commands(name, args.jobs, ROOT, source["selection"], env)]
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.error(str(error))
    environment = {name: os.getenv(name) for name in (
        "SDKROOT", "DEVELOPER_DIR", "RUSTFLAGS", "RUSTDOCFLAGS", "PKG_CONFIG_PATH",
        "CARGO_INCREMENTAL", "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG",
        "CARGO_PROFILE_DEV_SPLIT_DEBUGINFO", "CARGO_PROFILE_TEST_SPLIT_DEBUGINFO",
    )}
    report = {"source": source, "tier": args.tier, "target_dir": str(target),
              "environment": environment, **plan, "commands": commands}
    missing = sorted({command["argv"][0] for command in commands
                      if shutil.which(command["argv"][0]) is None})
    report["missing_tools"] = missing
    print(json.dumps(report, indent=2), flush=True)
    if args.plan:
        return 0
    if missing:
        print("Required local tools are unavailable: " + ", ".join(missing), file=sys.stderr)
        return 1
    for name in plan["checks"]:
        try:
            code = run_check(name, args.jobs, ROOT, source["selection"], env)
        except OSError as error:
            print(f"Check {name} could not run: {error}", file=sys.stderr)
            return 1
        if code:
            return code
    if args.candidate:
        try:
            after = ci_local.inspect_source(ROOT, "candidate", args.base, args.head, args.full, args.event)
        except (OSError, ValueError, subprocess.CalledProcessError) as error:
            print(f"Candidate changed during validation: {error}", file=sys.stderr)
            return 1
        if after != source:
            print("Candidate changed during validation", file=sys.stderr)
            return 1
    print("Local checks passed; this is not cross-platform CI acceptance")
    if not selected:
        print("No affected product checks selected; committed changes need an explicit --base or --all")
    for item in plan["not_run"]:
        print("Not run: " + item)
    return 0


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
    for name, needed in (("lint", bool(lint_packages(selected))), ("host_mac", "host" in selected)):
        if product and name == "host_mac" and "host" not in required:
            continue
        job = results.get(name)
        if (not isinstance(job, dict) or job.get("result") not in ({"success"} if needed else {"success", "skipped"})):
            raise ValueError(f"{name}: required check did not pass")
    for name in jobs[1:]:
        result = results[name].get("result")
        if product and (name not in selected or result != "success"):
            raise ValueError(f"{name}: product prerequisite did not pass ({result})")
        if result != "success" and (name in selected or result != "skipped"):
            raise ValueError(f"{name}: required check did not pass ({result})")


def cache_sources(mode, workspace, home):
    archive = workspace / ".cargo-sources"
    if mode == "save":
        if archive.is_symlink() or home.resolve().is_relative_to(archive.resolve()):
            raise ValueError("Source staging must be separate from Cargo home")
        if archive.exists():
            # Restored Git packs are read-only. Rebuild only this tool's scratch tree.
            for path in archive.rglob("*"):
                if not path.is_symlink():
                    path.chmod(0o755 if path.is_dir() else 0o644)
            archive.chmod(0o755)
            shutil.rmtree(archive)
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
    message = f"Repository cache: {size / 2**30:.3f} GiB ({count} entries)"
    print(message)
    if os.getenv("GITHUB_STEP_SUMMARY"):
        with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a", encoding="utf-8") as output:
            output.write(message + "\n\n")


def main():
    if len(sys.argv) >= 2 and sys.argv[1] == "local":
        raise SystemExit(local(sys.argv[2:]))
    elif len(sys.argv) == 3 and sys.argv[1] == "check":
        try:
            selection = json.loads(os.environ["CHECK_SCOPE"]) if sys.argv[2] == "lint" else None
            raise SystemExit(run_check(sys.argv[2], os.getenv("BUILD_JOBS", os.getenv("CARGO_BUILD_JOBS", "2")), selection=selection))
        except (ValueError, OSError, KeyError, subprocess.CalledProcessError) as error:
            raise SystemExit(str(error)) from error
    elif len(sys.argv) == 3 and sys.argv[1] == "source-cache" and sys.argv[2] in {"save", "restore"}:
        cache_sources(sys.argv[2], Path(os.environ["GITHUB_WORKSPACE"]),
                      Path(os.getenv("CARGO_HOME", str(Path.home() / ".cargo"))))
    elif len(sys.argv) == 3 and sys.argv[1] == "source-key":
        key = source_cache_key(Path(sys.argv[2]))
        with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as target:
            target.write(f"key={key}\n")
        print(key)
    elif sys.argv[1:] == ["cache-usage"]:
        cache_usage()
    elif sys.argv[1:] == ["lint"]:
        try:
            raise SystemExit(lint(json.loads(os.environ["CHECK_SCOPE"]), os.environ["BUILD_JOBS"]))
        except (ValueError, KeyError) as error:
            raise SystemExit(str(error)) from error
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
            "Usage: ci.py local [fast|affected] [options]|check <name>|source-key <lockfile>|source-cache <save|restore>|cache-usage|select|lint|report [desktop|host|mobile]|fixtures <owner>|"
            "speech-key <target> <shared|static>"
        )


if __name__ == "__main__":
    main()
