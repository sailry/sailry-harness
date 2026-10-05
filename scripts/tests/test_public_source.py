"""Publication checks use synthetic repositories, never private profiles."""

import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("public_source", ROOT / "scripts/public-source.py")
public = importlib.util.module_from_spec(spec)
spec.loader.exec_module(public)


def write(root, name, text):
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def initialize(root):
    public.git(root, "init", "--initial-branch=main", "--template=")
    public.git(root, "config", "user.name", "Fixture")
    public.git(root, "config", "user.email", "fixture@example.invalid")
    for name in public.REQUIRED:
        write(root, name, "# Public source\n" if name.endswith(".md") else "")


def commit(root, message):
    public.git(root, "add", "--all")
    public.git(root, "commit", "-m", message)


class PublicationPaths(unittest.TestCase):
    def test_preserves_guides_skills_licenses_and_lockfiles(self):
        paths = public.REQUIRED | {
            "plugins/context7/skills/docs/SKILL.md", "vendor/gpui/docs/contexts.md",
            "plugins/example/README.md", "third_party_licenses/ghostty-MIT.txt",
            ".cargo/config.toml", "services/pairing-relay/pnpm-lock.yaml", ".env.example",
        }
        self.assertEqual(public.validate_paths(paths), [])

    def test_rejects_private_and_generated_material(self):
        for path in (
            "docs/ARCHITECTURE.md", "plan/task.md", "output/report.md",
            ".runtime/profile/node.sqlite3", ".sailry/storage/node.sqlite3",
            ".migration-backups/archive.tar", "plugins/test/node_modules/module.js",
            ".env", ".env.production", "local/private.key", ".npmrc",
            "scripts/snapshot-migration.sh", "../outside.txt",
        ):
            with self.subTest(path=path):
                self.assertEqual(public.validate_paths(public.REQUIRED | {path}),
                                 [f"Non-public tracked path: {path}"])

    def test_ignore_rules_keep_public_nested_docs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            initialize(root)
            write(root, ".gitignore", (ROOT / ".gitignore").read_text())
            for path in ("docs/secret.md", "output/private.md", ".env", "plan/task.md",
                         "apps/mobile/ios/Flutter/Generated.xcconfig"):
                with self.subTest(path=path):
                    self.assertEqual(public.git(root, "check-ignore", path).decode().strip(), path)
            for path in ("AGENTS.md", "ARCHITECTURE.md", "plugins/context7/skills/docs/SKILL.md",
                         "vendor/gpui/docs/contexts.md", ".cargo/config.toml", ".env.example"):
                with self.subTest(path=path):
                    result = subprocess.run(["git", "-C", str(root), "check-ignore", path],
                                            capture_output=True)
                    self.assertEqual(result.returncode, 1)


class GuideLinks(unittest.TestCase):
    def test_localized_readme_keeps_publication_checks(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root, "README.md", "[Chinese](README.zh-CN.md)\n")
            write(root, "README.zh-CN.md", "# 简体中文\n[English](README.md)\n")
            paths = {"README.md", "README.zh-CN.md"}
            self.assertEqual(public.check_documents(root, paths), [])
            write(root, "README.zh-CN.md", "# 简体中文\n[Private](docs/secret.md)\n")
            write(root, "docs/secret.md", "Private\n")
            self.assertEqual(public.check_documents(root, paths), [
                "Guide link has no public source target: README.zh-CN.md: docs/secret.md",
            ])
            write(root, "README.zh-CN.md", "/Users/fixture/private\n")
            self.assertEqual(public.check_documents(root, paths), [
                "Non-public or non-English guide text: README.zh-CN.md",
            ])

    def test_checks_tracked_targets_not_private_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root, "README.md", "[Public](AGENTS.md) [Private](docs/secret.md)\n")
            write(root, "AGENTS.md", "# Rules\n")
            write(root, "docs/secret.md", "Private\n")
            self.assertEqual(public.check_documents(root, {"README.md", "AGENTS.md"}),
                             ["Guide link has no public source target: README.md: docs/secret.md"])

    def test_examples_and_remote_links_are_not_local_navigation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root, "README.md", "[Web](https://example.invalid)\n"
                  "```md\n[Example](output/report.docx)\n```\n`[Example](missing.md)`\n")
            self.assertEqual(public.check_documents(root, {"README.md"}), [])

    def test_rejects_repository_escape_and_non_english_copy(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write(root, "README.md", "内部指南 [Outside](../private.md)\n")
            self.assertEqual(public.check_documents(root, {"README.md"}), [
                "Non-public or non-English guide text: README.md",
                "Guide link escapes repository: README.md: ../private.md",
            ])


class SnapshotExport(unittest.TestCase):
    def test_preserves_pinned_submodule_without_copying_its_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "source"
            root.mkdir()
            initialize(root)
            module = Path(directory) / "packages"
            module.mkdir()
            initialize(module)
            write(module, "README.md", "# Packages\n")
            write(module, "package/plugin.json", '{}\n')
            commit(module, "Reviewed package")
            pinned = public.git(module, "rev-parse", "HEAD").decode().strip()
            with patch.dict(os.environ, {"GIT_ALLOW_PROTOCOL": "file"}):
                public.git(root, "submodule", "add", str(module), "plugins")
                write(root, "README.md", "[Packages](plugins/README.md)\n")
                commit(root, "Reviewed application")
                write(module, "new.txt", "Not selected\n")
                commit(module, "Later package")
                destination = Path(directory) / "public"
                public.export_source(root, destination)
            self.assertEqual(public.git(destination / "plugins", "rev-parse", "HEAD").decode().strip(), pinned)
            self.assertEqual(public.git(destination, "ls-files", "plugins"), b"plugins\n")
            self.assertEqual((destination / "plugins/package/plugin.json").read_text(), '{}\n')
            self.assertFalse((destination / "plugins/new.txt").exists())
            self.assertIn("plugins/package/plugin.json", public.check_source(destination))

    def test_exports_committed_source_without_private_history(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "source"
            root.mkdir()
            initialize(root)
            write(root, "docs/internal.md", "Private history\n")
            commit(root, "Private development")
            old = public.git(root, "rev-parse", "HEAD").decode().strip()
            public.git(root, "rm", "docs/internal.md")
            write(root, ".gitignore", "/docs/\n/output/\n")
            write(root, "public.txt", "Committed\n")
            commit(root, "Reviewed source")
            source_head = public.git(root, "rev-parse", "HEAD")
            write(root, "public.txt", "Uncommitted\n")
            write(root, "output/private.txt", "Ignored\n")
            write(root, "untracked.txt", "Untracked\n")
            destination = Path(directory) / "public"
            self.assertEqual(public.export_source(root, destination), destination)
            self.assertEqual((destination / "public.txt").read_text(), "Committed\n")
            self.assertFalse((destination / "docs").exists())
            self.assertFalse((destination / "output").exists())
            self.assertFalse((destination / "untracked.txt").exists())
            self.assertEqual(public.git(destination, "rev-list", "--all", "--count"), b"1\n")
            self.assertEqual(public.git(destination, "remote"), b"")
            result = subprocess.run(["git", "-C", str(destination), "cat-file", "-e", old],
                                    capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(public.git(root, "rev-parse", "HEAD"), source_head)
            self.assertEqual((root / "public.txt").read_text(), "Uncommitted\n")

    def test_rejects_non_public_snapshot_before_creating_destination(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "source"
            root.mkdir()
            initialize(root)
            write(root, "output/private.md", "Private\n")
            commit(root, "Internal source")
            destination = Path(directory) / "public"
            with self.assertRaisesRegex(ValueError, "Non-public tracked path"):
                public.export_source(root, destination)
            self.assertFalse(destination.exists())

    def test_does_not_overwrite_existing_destination(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "source"
            root.mkdir()
            initialize(root)
            commit(root, "Public source")
            destination = Path(directory) / "public"
            destination.mkdir()
            write(destination, "keep.txt", "Preserve\n")
            with self.assertRaises(FileExistsError):
                public.export_source(root, destination)
            self.assertEqual(list(destination.iterdir()), [destination / "keep.txt"])
            self.assertEqual((destination / "keep.txt").read_text(), "Preserve\n")


class Repository(unittest.TestCase):
    def test_public_source(self):
        self.assertTrue(public.check_source(ROOT))


if __name__ == "__main__":
    unittest.main()
