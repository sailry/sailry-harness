"""Affected CI checks use PR revisions and conservative dependency boundaries."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


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
            ("apps/host/src/main.rs", {"backend"}),
            ("crates/node-runtime/src/store.rs", {"backend", "desktop", "mobile"}),
            ("crates/embedding/Cargo.toml", {"backend", "desktop", "mobile"}),
            ("crates/ocr/src/lib.rs", {"backend", "desktop", "mobile"}),
            ("crates/protocol/Cargo.toml", ci.NATIVE),
            ("crates/link/src/lib.rs", ci.NATIVE),
            ("crates/client/src/lib.rs", ci.NATIVE),
            ("crates/speech/build.rs", ci.NATIVE),
            ("crates/model-assets/src/lib.rs", ci.NATIVE),
            ("crates/mobile-bridge/src/lib.rs", {"mobile", "android"}),
            ("apps/mobile/pubspec.lock", {"mobile", "android"}),
            ("tests/mobile-contract/lib/client.dart", {"mobile"}),
            ("services/pairing-relay/pnpm-lock.yaml", {"services"}),
            ("services/pairing-relay/src/server.ts", {"services"}),
            ("plugins", ci.NATIVE),
            ("plugins/code/skills/python/SKILL.md", ci.NATIVE),
            ("crates/node-runtime/src/agent/prompt.txt", {"backend", "desktop", "mobile"}),
            ("sdk/api.d.ts", ci.NATIVE),
            ("schemas/plugins/v1/extension.schema.json", ci.NATIVE),
        )
        for path, expected in examples:
            with self.subTest(path=path):
                self.assertEqual(ci.select_paths([path]), expected)

    def test_combines_changes(self):
        paths = ["apps/desktop/src/main.rs", "services/pairing-relay/package.json", "README.md"]
        self.assertEqual(ci.select_paths(paths), {"desktop", "services"})

    def test_skips_only_known_documentation(self):
        self.assertEqual(ci.select_paths([]), set())
        self.assertEqual(ci.select_paths(ci.DOCS | {"third_party_licenses/gpui.md", "sdk/plugins.md"}), set())
        for path in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml",
                     ".config/nextest.toml", ".gitmodules", ".github/workflows/ci.yml", "justfile",
                     "scripts/check-mobile-boundary.sh", "new-owner/README.md", "assets/new.png",
                     "vendor/adk-model/src/lib.rs", "locales/application.en.yml"):
            with self.subTest(path=path):
                self.assertEqual(ci.select_paths([path]), ci.LANES)

    def test_keeps_nonempty_matrix_when_skipped(self):
        for selected, kinds in ((set(), ["backend"]), ({"desktop"}, ["desktop"]),
                                ({"backend"}, ["backend"]), (ci.LANES, ["backend", "desktop"])):
            with self.subTest(selected=selected):
                output = ci.outputs(selected)
                self.assertEqual(json.loads(output["rust_matrix"]), {"kind": kinds})
                self.assertEqual(output["rust"], "true" if selected & {"backend", "desktop"} else "false")

    def test_push_and_manual_runs_are_full(self):
        for event in ("push", "workflow_dispatch", None):
            with self.subTest(event=event):
                self.assertEqual(ci.select_event(event, None), ci.LANES)


class Revisions(unittest.TestCase):
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
            self.assertEqual(ci.select_event("pull_request", payload, root), {"backend", "desktop"})

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


if __name__ == "__main__":
    unittest.main()
