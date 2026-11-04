"""Affected CI checks use event revisions and conservative dependency boundaries."""

import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("ci", ROOT / "scripts/ci.py")
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)


def git(root, *args):
    return subprocess.run(["git", *args], cwd=root, check=True, text=True, capture_output=True).stdout.strip()


def write(root, path):
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text("Fixture\n", encoding="utf-8")


def commit(root):
    git(root, "add", "--all")
    git(root, "commit", "-m", "Fixture")
    return git(root, "rev-parse", "HEAD")


class Selection(unittest.TestCase):
    def test_preserves_dependency_lanes(self):
        examples = (
            ("apps/desktop/src/pages.rs", {"desktop"}),
            ("crates/content/assets/visuals/render.js", {"desktop"}),
            ("apps/host/src/main.rs", {"host"}),
            ("tools/release/src/main.rs", {"shared"}),
            ("crates/node-runtime/src/store.rs", {"shared", "desktop", "host", "mobile_ffi"}),
            ("crates/embedding/Cargo.toml", {"shared", "desktop", "host", "mobile_ffi"}),
            ("crates/ocr/src/lib.rs", {"shared", "desktop", "host", "mobile_ffi"}),
            ("crates/decision/src/lib.rs", {"shared", "desktop", "host", "mobile_ffi"}),
            ("crates/protocol/Cargo.toml", ci.NATIVE),
            ("crates/link/src/lib.rs", ci.NATIVE),
            ("crates/client/src/lib.rs", ci.NATIVE),
            ("crates/speech/build.rs", ci.NATIVE),
            ("crates/model-assets/src/lib.rs", ci.NATIVE),
            ("crates/mobile-bridge/src/lib.rs", {"mobile", "mobile_ffi", "android"}),
            ("apps/mobile/pubspec.lock", {"mobile", "android"}),
            ("vendor/flutter-markdown-plus/lib/src/builder.dart", {"mobile", "android"}),
            ("vendor/flutter-markdown-plus/pubspec.yaml", {"mobile", "android"}),
            ("vendor/flutter-markdown-plus/LICENSE", {"mobile", "android"}),
            ("vendor/flutter-markdown-plus/SAILRY.md", {"mobile", "android"}),
            ("tests/mobile-contract/lib/client.dart", {"mobile_ffi"}),
            ("services/pairing-relay/pnpm-lock.yaml", {"services"}),
            ("services/pairing-relay/src/server.ts", {"services"}),
            ("plugins", ci.NATIVE),
            ("plugins/code/skills/python/SKILL.md", ci.NATIVE),
            ("scripts/package/code-parsers.py", {"shared", "desktop", "host", "mobile_ffi"}),
            ("scripts/package/code-parsers.lock.json", {"shared", "desktop", "host", "mobile_ffi"}),
            ("themes", {"desktop"}),
            ("themes/catalog.json", {"desktop"}),
            ("themes/great-sage/theme.json", {"desktop"}),
            ("crates/node-runtime/src/agent/prompt.txt", {"shared", "desktop", "host", "mobile_ffi"}),
            ("sdk/api.d.ts", ci.NATIVE),
            ("schemas/plugins/v1/extension.schema.json", ci.NATIVE),
        )
        for path, expected in examples:
            with self.subTest(path=path):
                self.assertEqual(ci.select_paths([path]), expected)

    def test_combines_changes(self):
        paths = ["apps/desktop/src/main.rs", "services/pairing-relay/package.json", "README.md"]
        self.assertEqual(ci.select_paths(paths), {"desktop", "services"})

    def test_standalone_node_tests_select_shared(self):
        for path in (
            "agent.rs", "agent/delegation/guidance.rs", "agent/providers/native/retry.rs",
            "suites/resources.rs", "memory.rs", "memory/archive.rs", "new_feature/case.rs",
        ):
            with self.subTest(path=path):
                self.assertEqual(ci.select_paths([f"crates/node-runtime/tests/{path}"]), {"shared"})
        self.assertEqual(ci.select_paths([
            "crates/node-runtime/tests/agent/delegation/guidance.rs", "apps/mobile/lib/main.dart",
        ]), {"shared", "mobile", "android"})
        self.assertEqual(ci.select_paths([
            "crates/node-runtime/tests/agent/delegation/guidance.rs", "crates/node-runtime/src/agent.rs",
        ]), {"shared", "desktop", "host", "mobile_ffi"})

    def test_exported_fixtures_preserve_consumers(self):
        for path in (
            "agent_support/new_reply.rs", "discovery_support/mod.rs", "support/database/native.rs",
            "ssh/server/process.rs", "databases/servers.rs", "logging/parse.rs", "plugins/support.rs",
            "ports/server.rs", "agent/providers/native/server.rs", "agent/providers/native/server/frames.rs",
        ):
            with self.subTest(path=path):
                self.assertEqual(ci.select_paths([f"crates/node-runtime/tests/{path}"]),
                                 {"shared", "desktop", "host", "mobile_ffi"})

    def test_covers_actual_fixture_imports(self):
        fixture_root = ROOT / "crates/node-runtime/tests"
        for consumer, lane in (("apps/desktop", "desktop"), ("apps/host", "host"),
                               ("crates/mobile-bridge", "mobile_ffi")):
            imports = set()
            for source in (ROOT / consumer).rglob("*.rs"):
                for relative in re.findall(r'#\[path\s*=\s*"([^"]+)"\]', source.read_text()):
                    target = (source.parent / relative).resolve()
                    if target.is_relative_to(fixture_root):
                        imports.add(target.relative_to(ROOT).as_posix())
            self.assertTrue(imports, consumer)
            for path in imports:
                with self.subTest(consumer=consumer, path=path):
                    self.assertIn(lane, ci.select_paths([path]))

    def test_release_changes_use_source_checks(self):
        paths = (
            ".github/workflows/release-version.yml", ".github/workflows/release.yml",
            ".github/workflows/release-host.yml", ".github/workflows/release-mobile.yml",
            ".github/workflows/release-desktop-formal.yml", ".github/workflows/changelog.yml",
            ".github/workflows/update-feed.yml",
            "scripts/start-release.py", "scripts/release-notes.py", "scripts/check-release.py",
            "scripts/publish-packages.py", "scripts/publish-update-feed.py",
            "scripts/package/publication.py", "scripts/package/mobile-release.py", "scripts/package/ios-signing.py",
            "scripts/tests/test_mobile_release.py", "scripts/tests/test_publish_update_feed.py",
        )
        for path in paths:
            with self.subTest(path=path):
                self.assertEqual(ci.select_paths([path]), set())
        self.assertEqual(ci.select_paths(paths), set())

    def test_release_changes_preserve_product_selection(self):
        for path, expected in (
            ("apps/desktop/src/main.rs", {"desktop"}),
            ("apps/host/src/main.rs", {"host"}),
            ("apps/mobile/lib/main.dart", {"mobile", "android"}),
            ("crates/node-runtime/src/store.rs", {"shared", "desktop", "host", "mobile_ffi"}),
        ):
            with self.subTest(path=path):
                self.assertEqual(ci.select_paths([*ci.SOURCE_ONLY, path]), expected)

    def test_release_whitelist_excludes_build_inputs(self):
        for path in (
            "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml",
            ".github/workflows/ci.yml", ".github/workflows/release-new.yml",
            ".github/workflows/release-mobile.yml.backup", ".github/actions/rust-cache/action.yml",
            ".github/actions/native-cache/action.yml", "scripts/package/versions.py",
            "scripts/package/mobile-build.py", "scripts/check-mobile-android.sh",
        ):
            with self.subTest(path=path):
                self.assertEqual(ci.select_paths([*ci.SOURCE_ONLY, path]), ci.LANES)

    def test_selects_registered_fixture_owners(self):
        for owner, names in ci.FIXTURES.items():
            for name in names:
                with self.subTest(owner=owner, name=name):
                    self.assertEqual(ci.select_paths([f"scripts/tests/{name}.py"]),
                                     set() if owner == "shared" else {owner})
        self.assertEqual(ci.select_paths(["scripts/tests/test_names.py", "apps/host/src/main.rs"]),
                         {"host"})
        for path in ("scripts/tests/test_new_owner.py", "scripts/test_names.py", "scripts/ci.py"):
            with self.subTest(path=path):
                self.assertEqual(ci.select_paths([path]), ci.LANES)

    def test_skips_only_known_documentation(self):
        self.assertEqual(ci.select_paths([]), set())
        self.assertEqual(ci.select_paths(["CLA.md", "sdk/LICENSE"]), set())
        self.assertEqual(ci.select_paths(["sdk/src/index.ts"]), ci.NATIVE)
        self.assertEqual(ci.select_paths(["update-other.json"]), ci.LANES)
        self.assertEqual(ci.select_paths(ci.DOCS | {"third_party_licenses/gpui.md", "sdk/plugins.md"}), set())
        self.assertEqual(ci.select_paths(["assets/readme/workspace-dark.png"]), set())
        for path in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml",
                     ".config/nextest.toml", ".gitmodules", ".github/workflows/ci.yml", "justfile",
                     "scripts/check-mobile-boundary.sh", "new-owner/README.md", "assets/new.png",
                     "vendor/new-crate/src/lib.rs", "locales/application.en.yml"):
            with self.subTest(path=path):
                self.assertEqual(ci.select_paths([path]), ci.LANES)

    def test_reports_each_selected_lane(self):
        for selected in (set(), {"desktop"}, {"shared"}, {"host"}, ci.LANES):
            with self.subTest(selected=selected):
                output = ci.outputs(selected)
                self.assertEqual(set(output), ci.LANES)
                self.assertEqual({name for name, value in output.items() if value == "true"}, selected)

    def test_unavailable_and_manual_runs_are_full(self):
        for event in ("push", "workflow_dispatch", None):
            with self.subTest(event=event):
                self.assertEqual(ci.select_event(event, None), ci.LANES)


class Revisions(unittest.TestCase):
    def test_push_reads_both_ends(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git(root, "init", "--initial-branch=main", "--template=")
            git(root, "config", "user.name", "Fixture")
            git(root, "config", "user.email", "fixture@example.invalid")
            write(root, "README.md")
            write(root, "apps/host/src/main.rs")
            base = commit(root)
            (root / "README.md").write_text("Updated\n")
            docs = commit(root)
            payload = root / ".git" / "event.json"
            event = {"ref": "refs/heads/main", "before": base, "after": docs,
                     "created": False, "deleted": False, "forced": False}
            payload.write_text(json.dumps(event))
            self.assertEqual(ci.select_event("push", payload, root), set())
            (root / "apps/host/src/main.rs").unlink()
            write(root, "apps/desktop/src/main.rs")
            head = commit(root)
            event["after"] = head
            payload.write_text(json.dumps(event))
            self.assertEqual(ci.select_event("push", payload, root), {"desktop", "host"})
            for change in ({"before": "0" * 40}, {"after": docs}, {"forced": True},
                           {"created": True}, {"deleted": True}, {"ref": "refs/heads/other"},
                           {"before": "f" * 40}):
                with self.subTest(change=change):
                    payload.write_text(json.dumps({**event, **change}))
                    self.assertEqual(ci.select_event("push", payload, root), ci.LANES)
            event.update(before=head, after=base)
            git(root, "checkout", "--detach", base)
            payload.write_text(json.dumps(event))
            self.assertEqual(ci.select_event("push", payload, root), ci.LANES)

    def test_reads_pr_merge_base_and_rename_sides(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git(root, "init", "--initial-branch=main", "--template=")
            git(root, "config", "user.name", "Fixture")
            git(root, "config", "user.email", "fixture@example.invalid")
            old = "apps/host/src/main.rs"
            deleted = "crates/content/src/removed.rs"
            write(root, old)
            write(root, deleted)
            commit(root)
            git(root, "checkout", "-b", "feature")
            new = "apps/desktop/src/main.rs"
            (root / new).parent.mkdir(parents=True)
            (root / old).rename(root / new)
            (root / deleted).unlink()
            unusual = "apps/desktop/src/space and\nnewline.rs"
            write(root, unusual)
            head = commit(root)
            git(root, "checkout", "main")
            write(root, "apps/mobile/lib/main.dart")
            base = commit(root)
            payload = root / "event.json"
            payload.write_text(json.dumps({"pull_request": {"base": {"sha": base}, "head": {"sha": head}}}))
            self.assertEqual(set(ci.changed_paths(root, base, head)), {old, deleted, new, unusual})
            self.assertEqual(ci.select_event("pull_request", payload, root), {"host", "desktop"})

    def test_unavailable_changes_require_every_lane(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payload = root / "event.json"
            for value in ({}, {"pull_request": {"base": {"sha": "--help"}, "head": {"sha": "0" * 40}}},
                          {"pull_request": {"base": {"sha": "0" * 40}, "head": {"sha": "1" * 40}}}):
                payload.write_text(json.dumps(value))
                self.assertEqual(ci.select_event("pull_request", payload, root), ci.LANES)
            payload.write_text("invalid JSON")
            self.assertEqual(ci.select_event("pull_request", payload, root), ci.LANES)
            self.assertEqual(ci.select_event("pull_request", None, root), ci.LANES)
            self.assertEqual(ci.select_event("pull_request", root / "missing.json", root), ci.LANES)

    def test_emits_github_outputs(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "outputs"
            result = subprocess.run(
                [sys.executable, str(ROOT / "scripts/ci.py"), "select"],
                env={**os.environ, "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_OUTPUT": str(output)},
                text=True, capture_output=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            actual = dict(line.split("=", 1) for line in output.read_text().splitlines())
            self.assertEqual(actual, {**ci.outputs(ci.LANES), "mode": "full",
                                      "light_linux": "false", "light_macos": "false"})


class Modes(unittest.TestCase):
    def mode(self, payload, event="pull_request"):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "event.json"
            path.write_text(json.dumps(payload))
            return ci.event_mode(event, path)

    def request(self, action="synchronize", draft=False, labels=(), label=None):
        payload = {"action": action, "pull_request": {"draft": draft, "labels": [{"name": name} for name in labels]}}
        if label is not None:
            payload["label"] = {"name": label}
        return payload

    def test_maps_pull_request_state(self):
        full = ci.FULL_LABEL
        for payload, expected in (
            (self.request(), "light"),
            (self.request("opened"), "light"),
            (self.request("reopened", labels=["bug"]), "light"),
            (self.request("ready_for_review"), "light"),
            (self.request(labels=[full]), "full"),
            (self.request("ready_for_review", labels=[full]), "full"),
            (self.request("labeled", labels=[full], label=full), "full"),
            (self.request("labeled", labels=[full, "bug"], label="bug"), "skip"),
            (self.request("labeled", labels=["bug"], label="bug"), "skip"),
            (self.request(draft=True), "skip"),
            (self.request(draft=True, labels=[full]), "skip"),
            (self.request("labeled", draft=True, labels=[full], label=full), "skip"),
        ):
            with self.subTest(payload=payload):
                self.assertEqual(self.mode(payload), expected)

    def test_other_events_and_unreadable_state_are_full(self):
        for event in ("push", "workflow_dispatch", None):
            self.assertEqual(ci.event_mode(event, None), "full")
        for payload in ({}, {"pull_request": {"labels": []}}, {"pull_request": {"draft": False}},
                        {"action": "labeled", "pull_request": {"draft": False, "labels": []}}):
            with self.subTest(payload=payload):
                self.assertEqual(self.mode(payload), "full")
        self.assertEqual(ci.event_mode("pull_request", None), "full")

    def test_deferred_runs_select_nothing(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "event.json"
            path.write_text(json.dumps(self.request(draft=True)))
            with patch.object(ci, "select_event") as select:
                result = ci.select_outputs("pull_request", path)
            select.assert_not_called()
            self.assertEqual(result, {**ci.outputs(set()), "mode": "skip",
                                      "light_linux": "false", "light_macos": "false"})

    def test_light_outputs_follow_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "event.json"
            path.write_text(json.dumps(self.request()))
            for selected, linux, macos in (({"host"}, "true", "false"), ({"desktop"}, "true", "true"),
                                           ({"services"}, "false", "false"), (set(), "false", "false")):
                with self.subTest(selected=selected), patch.object(ci, "select_event", return_value=selected):
                    result = ci.select_outputs("pull_request", path)
                    self.assertEqual(result, {**ci.outputs(selected), "mode": "light",
                                              "light_linux": linux, "light_macos": macos})


class Lint(unittest.TestCase):
    def test_splits_packages_by_platform(self):
        linux, macos = ci.lint_packages(ci.LANES, "linux"), ci.lint_packages(ci.LANES, "macos")
        self.assertEqual(sorted(linux + macos), ci.lint_packages(ci.LANES))
        self.assertEqual(macos, sorted(ci.MACOS_LINT))
        self.assertIn("sailry-node-runtime", linux)
        self.assertIn("sailry-host", linux)
        self.assertEqual(ci.lint_packages({"shared"}, "macos"), ["sailry-speech"])
        self.assertEqual(ci.lint_packages({"desktop"}, "linux"), [])
        command = ci.lint_command(ci.outputs(ci.LANES), "2", "linux")
        self.assertEqual(command[command.index("--features") + 1], "sailry-node-runtime/test-support")
        self.assertEqual(command[command.index("--all-targets"):],
                         ["--all-targets", "--jobs", "2", "--", "-D", "warnings"])
        self.assertIsNone(ci.lint_command(ci.outputs({"desktop"}), None, "linux"))
        with self.assertRaises(ValueError):
            ci.lint_command(ci.outputs({"desktop"}), None, "windows")
        with patch.object(ci.subprocess, "run") as run, patch("builtins.print"):
            self.assertEqual(ci.lint(ci.outputs({"desktop"}), system="linux"), 0)
            run.assert_not_called()

    def test_reads_lane_scope_with_mode(self):
        scope = {**ci.outputs({"host"}), "mode": "light", "light_linux": "true", "light_macos": "false"}
        for argv, system in ((["ci.py", "lint"], None), (["ci.py", "lint", "linux"], "linux")):
            with self.subTest(argv=argv), patch.dict(os.environ, {"CHECK_SCOPE": json.dumps(scope)}), \
                    patch.object(sys, "argv", argv), patch.object(ci, "lint", return_value=0) as lint:
                with self.assertRaises(SystemExit) as outcome:
                    ci.main()
                self.assertEqual(outcome.exception.code, 0)
                lint.assert_called_once_with(ci.outputs({"host"}), system=system)

    def test_selects_existing_package_union(self):
        self.assertEqual(ci.lint_packages(set()), [])
        self.assertEqual(ci.lint_packages({"host"}), ["sailry-host"])
        self.assertEqual(ci.lint_packages({"mobile_ffi", "android", "services"}), [])
        expected = {package for packages in ci.LINT_PACKAGES.values() for package in packages}
        self.assertEqual(ci.lint_packages(ci.LANES), sorted(expected))
        self.assertEqual(ci.lint_packages({"desktop", "host"}),
                         ["sailry-content", "sailry-desktop", "sailry-host"])

    def test_runs_one_strict_command(self):
        with patch.object(ci, "parser_environment", return_value={}) as prepare, \
                patch.object(ci.subprocess, "run") as run:
            run.return_value.returncode = 1
            self.assertEqual(ci.lint(ci.outputs({"host"}), "1"), 1)
            run.assert_called_once_with(
                ["cargo", "clippy", "--locked", "-p", "sailry-host", "--all-targets",
                 "--jobs", "1", "--", "-D", "warnings"], env={}, check=False)
            prepare.assert_called_once_with(ROOT, None, libraries=False)
        shared = ci.lint_command(ci.outputs({"shared"}), None)
        self.assertEqual(shared[shared.index("--features") + 1], "sailry-node-runtime/test-support")
        self.assertLess(shared.index("--features"), shared.index("--all-targets"))
        for selection, jobs in (({}, "1"), (ci.outputs(set()), "1"),
                                (ci.outputs({"host"}), "0"), (ci.outputs({"host"}), "1; false")):
            with self.subTest(selection=selection, jobs=jobs):
                with self.assertRaises(ValueError), patch.object(ci.subprocess, "run") as run:
                    ci.lint(selection, jobs)
                run.assert_not_called()


class Commands(unittest.TestCase):
    def test_skips_fixture_backoff_without_mutating_environment(self):
        for configured, expected in (({}, "true"), ({"GOOSE_PROVIDER_SKIP_BACKOFF": "false"}, "false")):
            environment = {"FIXTURE_SETTING": "preserved", **configured}
            with patch.object(ci, "check_commands", return_value=[{"argv": ["fixture"], "cwd": "."}]), \
                    patch.object(ci, "parser_environment", side_effect=lambda root, env, libraries: dict(env)), \
                    patch.object(ci.subprocess, "run") as run:
                run.return_value.returncode = 0
                self.assertEqual(ci.run_check("backend", env=environment), 0)
                self.assertEqual(run.call_args.kwargs["env"], {**environment, "GOOSE_PROVIDER_SKIP_BACKOFF": expected})
                self.assertEqual(environment, {"FIXTURE_SETTING": "preserved", **configured})

    def test_uses_cargo_default_locally(self):
        for name in ("backend", "desktop", "host", "desktop-production", "host-production",
                     "desktop-lifecycle", "host-mac", "databases", "mobile-contract", "lint", "tests"):
            with self.subTest(name=name):
                for command in ci.check_commands(name, selection=ci.outputs(ci.LANES)):
                    self.assertNotIn("--jobs", command["argv"])
                    self.assertNotIn("--build-jobs", command["argv"])
                    self.assertNotIn(None, command["argv"])

    def test_preserves_explicit_build_limits(self):
        for name in ("backend", "desktop", "host", "desktop-production", "host-production", "lint", "tests"):
            with self.subTest(name=name):
                commands = ci.check_commands(name, "14", selection=ci.outputs(ci.LANES))
                for command in commands:
                    arguments = command["argv"]
                    if arguments[0] != "cargo":
                        continue
                    flag = "--build-jobs" if arguments[1] == "nextest" else "--jobs"
                    self.assertEqual(arguments[arguments.index(flag) + 1], "14")

    def test_leaves_environment_limits_to_cargo(self):
        for value in ("2", "4", "-1", "default"):
            for is_ci in ("true", "false"):
                environment = {"CI": is_ci, "CARGO_BUILD_JOBS": value}
                with self.subTest(environment=environment), patch.dict(ci.os.environ, environment, clear=True):
                    for name in ("backend", "desktop", "host", "lint"):
                        for command in ci.check_commands(name, selection=ci.outputs(ci.LANES)):
                            self.assertNotIn("--jobs", command["argv"])
                            self.assertNotIn("--build-jobs", command["argv"])
                    self.assertEqual(ci.os.environ["CARGO_BUILD_JOBS"], value)

    def test_runs_real_plugin_contracts_after_backend(self):
        commands = ci.check_commands("backend", env={"CARGO_TARGET_DIR":"/fixture/lane", "CARGO_BUILD_TARGET":"aarch64-apple-darwin"})
        self.assertEqual(len(commands), 3)
        build = commands[1]["argv"]
        self.assertEqual(build[:3], ["cargo", "build", "--locked"])
        self.assertEqual(build[build.index("--features") + 1], "sailry-node-runtime/test-support")
        run = commands[0]["argv"]
        self.assertEqual(run[run.index("--features") + 1], "sailry-node-runtime/test-support")
        packages = lambda argv: [argv[index + 1] for index, value in enumerate(argv) if value == "-p"]
        self.assertEqual(packages(build), packages(commands[0]["argv"]))
        self.assertEqual(build[build.index("--example") + 1], "plugin-test-host")
        self.assertEqual(build[build.index("--profile") + 1], "test")
        self.assertEqual(commands[2]["argv"][1:], ["scripts/test-plugins.py", "--runtime-host", "/fixture/lane/aarch64-apple-darwin/debug/examples/plugin-test-host"])

    def test_reuses_backend_graph_and_profiles(self):
        backend = ci.check_commands("backend", "2")[0]
        databases = ci.check_commands("databases", "2")[0]
        packages = lambda argv: [argv[index + 1] for index, value in enumerate(argv) if value == "-p"]
        self.assertEqual(packages(backend["argv"]), list(ci.LINT_PACKAGES["shared"]))
        self.assertEqual(packages(backend["argv"]), packages(databases["argv"]))
        self.assertEqual(backend["argv"][backend["argv"].index("--profile") + 1], "ci")
        self.assertEqual(databases["argv"][databases["argv"].index("--profile") + 1], "databases")
        self.assertNotIn("--release", backend["argv"])
        self.assertNotIn("--ignored", backend["argv"])

    def test_preserves_working_directory_and_exit_status(self):
        with patch.object(ci.subprocess, "run") as run:
            run.return_value.returncode = 7
            self.assertEqual(ci.run_check("mobile-check", root=ROOT), 7)
            run.assert_called_once_with(["flutter", "analyze", "--no-pub"],
                                        cwd=ROOT / "apps/mobile", env=None, check=False)

    def test_fast_excludes_product_builds(self):
        plan = ci.local_checks(ci.LANES, "fast", "Darwin")
        for name in plan["checks"]:
            for command in ci.check_commands(name, "2"):
                self.assertFalse(command["argv"][:2] in (["cargo", "nextest"], ["cargo", "clippy"],
                                                       ["cargo", "check"], ["cargo", "build"]))
        self.assertTrue(plan["not_run"])

    def test_reports_platform_and_service_boundaries(self):
        plan = ci.local_checks(ci.LANES, "affected", "Darwin")
        self.assertIn("tests", plan["checks"])
        self.assertIn("host-production", plan["checks"])
        self.assertNotIn("databases", plan["checks"])
        self.assertTrue(any("database" in value for value in plan["not_run"]))
        windows = ci.local_checks({"desktop", "host", "android"}, "affected", "Windows")
        self.assertNotIn("tests", windows["checks"])
        self.assertNotIn("desktop-production", windows["checks"])
        self.assertTrue(any("Android" in value for value in windows["not_run"]))

    def test_builds_local_product_tests_once(self):
        plan = ci.local_checks(ci.LANES, "affected", "Darwin")
        for name in ("backend", "desktop", "desktop-lifecycle", "host", "host-mac"):
            self.assertNotIn(name, plan["checks"])
        commands = ci.check_commands("tests", selection=ci.outputs(ci.LANES), threads="7", system="Darwin")
        run, build = commands[0]["argv"], commands[1]["argv"]
        packages = lambda argv: [argv[index + 1] for index, value in enumerate(argv) if value == "-p"]
        expected = [*ci.LINT_PACKAGES["shared"], *ci.LINT_PACKAGES["desktop"], *ci.LINT_PACKAGES["host"]]
        self.assertEqual(packages(run), expected)
        self.assertEqual(packages(build), expected)
        self.assertIn("--test-threads=7", run)
        self.assertIn("binary(lifecycle)", run[run.index("--filterset") + 1])
        self.assertEqual(commands[2]["argv"][1], "scripts/test-plugins.py")
        linux = ci.check_commands("tests", selection=ci.outputs({"host"}), system="Linux")
        self.assertEqual(len(linux), 1)
        self.assertNotIn("--filterset", linux[0]["argv"])
        self.assertIn(f"--test-threads={ci.CI_TEST_THREADS}", linux[0]["argv"])
        with self.assertRaises(ValueError):
            ci.check_commands("tests", selection=ci.outputs({"desktop"}), system="Linux")

    def test_rejects_unknown_checks_and_invalid_jobs(self):
        for name, jobs in [("missing", "1"), ("backend", "0"), ("backend", "1; false")]:
            with self.subTest(name=name, jobs=jobs), self.assertRaises(ValueError):
                ci.check_commands(name, jobs)
        for threads in ("0", "4; false"):
            with self.subTest(threads=threads), self.assertRaises(ValueError):
                ci.check_commands("backend", threads=threads)


if __name__ == "__main__":
    unittest.main()
