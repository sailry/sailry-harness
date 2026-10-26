"""Affected CI checks use event revisions and conservative dependency boundaries."""

import importlib.util
import json
import os
from pathlib import Path
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
            self.assertEqual(actual, ci.outputs(ci.LANES))


class Lint(unittest.TestCase):
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
        for selection, jobs in (({}, "1"), (ci.outputs(set()), "1"),
                                (ci.outputs({"host"}), "0"), (ci.outputs({"host"}), "1; false")):
            with self.subTest(selection=selection, jobs=jobs):
                with self.assertRaises(ValueError), patch.object(ci.subprocess, "run") as run:
                    ci.lint(selection, jobs)
                run.assert_not_called()


class Commands(unittest.TestCase):
    def test_runs_real_plugin_contracts_after_backend(self):
        commands = ci.check_commands("backend", env={"CARGO_TARGET_DIR":"/fixture/lane", "CARGO_BUILD_TARGET":"aarch64-apple-darwin"})
        self.assertEqual(len(commands), 3)
        build = commands[1]["argv"]
        self.assertEqual(build[:3], ["cargo", "build", "--locked"])
        self.assertEqual(build[build.index("--features") + 1], "test-support")
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
        self.assertIn("host-mac", plan["checks"])
        self.assertNotIn("host", plan["checks"])
        self.assertNotIn("databases", plan["checks"])
        self.assertTrue(any("database" in value for value in plan["not_run"]))
        windows = ci.local_checks({"desktop", "host", "android"}, "affected", "Windows")
        self.assertNotIn("desktop", windows["checks"])
        self.assertNotIn("host", windows["checks"])
        self.assertTrue(any("Android" in value for value in windows["not_run"]))

    def test_rejects_unknown_checks_and_invalid_jobs(self):
        for name, jobs in [("missing", "1"), ("backend", "0"), ("backend", "1; false")]:
            with self.subTest(name=name, jobs=jobs), self.assertRaises(ValueError):
                ci.check_commands(name, jobs)


if __name__ == "__main__":
    unittest.main()
