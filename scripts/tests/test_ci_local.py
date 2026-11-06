"""Local selection distinguishes mutable development inputs from candidate commits."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import ci_local
sys.path.pop(0)


def git(root, *arguments):
    return subprocess.run(["git", *arguments], cwd=root, check=True,
                          capture_output=True, text=True).stdout.strip()


def initialize(root):
    git(root, "init", "--initial-branch=main", "--template=")
    git(root, "config", "user.name", "Fixture")
    git(root, "config", "user.email", "fixture@example.invalid")
    write(root, "README.md", "Fixture\n")
    return commit(root)


def write(root, name, text="Updated\n"):
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def commit(root):
    git(root, "add", "--all")
    git(root, "commit", "-m", "Fixture")
    return git(root, "rev-parse", "HEAD")


def lanes(report):
    return {name for name, value in report["selection"].items() if value == "true"}


class Worktree(unittest.TestCase):
    def test_combines_index_worktree_and_untracked(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            initialize(root)
            write(root, "apps/desktop/src/main.rs", "Original\n")
            commit(root)
            write(root, "apps/desktop/src/main.rs")
            git(root, "add", "apps/desktop/src/main.rs")
            # The final file matches HEAD, but both index differences still count.
            write(root, "apps/desktop/src/main.rs", "Original\n")
            write(root, "apps/host/src/new file.rs")
            report = ci_local.inspect_source(root, "worktree")
            self.assertEqual(report["paths"], ["apps/desktop/src/main.rs", "apps/host/src/new file.rs"])
            self.assertEqual(lanes(report), {"desktop", "host"})
            self.assertTrue(report["dirty"])
            self.assertTrue(report["inspection_only"])
            self.assertEqual(report["base"], report["head"])

    def test_includes_committed_changes_with_explicit_base(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            base = initialize(root)
            write(root, "apps/desktop/src/main.rs")
            head = commit(root)
            write(root, "apps/host/src/main.rs")
            report = ci_local.inspect_source(root, "worktree", base)
            self.assertEqual(lanes(report), {"desktop", "host"})
            self.assertEqual(report["head"], head)
            self.assertEqual(report["base"], base)

    def test_empty_inputs_do_not_claim_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            initialize(root)
            report = ci_local.inspect_source(root, "worktree")
            self.assertEqual(lanes(report), set())
            self.assertFalse(report["dirty"])
            self.assertIsNotNone(report["empty_reason"])
            self.assertTrue(report["inspection_only"])


class Candidate(unittest.TestCase):
    def test_binds_checkout_and_both_rename_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            initialize(root)
            write(root, "apps/desktop/src/main.rs")
            base = commit(root)
            (root / "apps/desktop/src/main.rs").unlink()
            write(root, "apps/host/src/main.rs")
            head = commit(root)
            report = ci_local.inspect_source(root, "candidate", base, head)
            self.assertEqual(lanes(report), {"desktop", "host"})
            self.assertEqual(report["head_tree"], git(root, "rev-parse", "HEAD^{tree}"))
            with self.assertRaisesRegex(ValueError, "checkout must match"):
                ci_local.inspect_source(root, "candidate", base, base)
            write(root, "untracked.txt")
            with self.assertRaisesRegex(ValueError, "clean checkout"):
                ci_local.inspect_source(root, "candidate", base, head)

    def test_equal_or_missing_revisions_require_explicit_scope(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            head = initialize(root)
            for base, candidate in [(None, head), (head, None), (head, head)]:
                with self.subTest(base=base, head=candidate), self.assertRaises(ValueError):
                    ci_local.inspect_source(root, "candidate", base, candidate)
            git(root, "update-ref", "refs/remotes/origin/main", head)
            with self.assertRaisesRegex(ValueError, "identical"):
                ci_local.inspect_source(root, "candidate", "origin/main", "HEAD")
            report = ci_local.inspect_source(root, "candidate", head=head, full=True)
            self.assertEqual(lanes(report), ci_local.ci.LANES)
            self.assertTrue(report["inspection_only"])

    def test_pr_compares_from_merge_base(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            initialize(root)
            git(root, "switch", "-c", "feature")
            write(root, "apps/desktop/src/main.rs")
            head = commit(root)
            git(root, "switch", "main")
            write(root, "apps/host/src/main.rs")
            base = commit(root)
            git(root, "switch", "feature")
            report = ci_local.inspect_source(root, "candidate", base, head, event="pull_request")
            self.assertEqual(lanes(report), {"desktop"})
            with self.assertRaises(subprocess.CalledProcessError):
                ci_local.inspect_source(root, "candidate", base, head)


class PrePush(unittest.TestCase):
    def plan(self, root, *arguments):
        with patch.object(ci_local.ci, "ROOT", root), \
                patch.object(ci_local.ci, "tool_versions", return_value={}), \
                patch("builtins.print") as output:
            code = ci_local.ci.local(["pre-push", "--plan", *arguments])
        return code, json.loads(output.call_args_list[0].args[0])

    def test_includes_commits_since_target_merge_base(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            initialize(root)
            git(root, "update-ref", "refs/remotes/origin/main", "HEAD")
            git(root, "switch", "--quiet", "-c", "feature")
            write(root, "apps/host/src/main.rs")
            commit(root)
            git(root, "switch", "--quiet", "main")
            write(root, "apps/desktop/src/main.rs")
            git(root, "update-ref", "refs/remotes/origin/main", commit(root))
            git(root, "switch", "--quiet", "feature")
            write(root, "docs/hosts.md")
            code, report = self.plan(root)
            self.assertEqual(code, 0)
            self.assertEqual(report["source"]["paths"], ["apps/host/src/main.rs", "docs/hosts.md"])
            self.assertEqual(lanes(report["source"]), {"host"})
            for check in ("format", "locales", "lint", "tests", "host-production"):
                self.assertIn(check, report["checks"])

    def test_fails_when_nothing_changed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            initialize(root)
            git(root, "update-ref", "refs/remotes/release", "HEAD")
            code, report = self.plan(root, "--target", "release")
            self.assertEqual(code, 1)
            self.assertEqual(report["source"]["paths"], [])
            self.assertEqual(report["source"]["base"], report["source"]["head"])

    def test_requires_known_target_without_other_baselines(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            initialize(root)
            for arguments in ((), ("--base", "HEAD"), ("--all",)):
                with self.subTest(arguments=arguments), patch("sys.stderr"), self.assertRaises(SystemExit):
                    self.plan(root, *arguments)
            with patch("sys.stderr"), self.assertRaises(SystemExit):
                with patch.object(ci_local.ci, "ROOT", root):
                    ci_local.ci.local(["affected", "--target", "main"])


class Submodules(unittest.TestCase):
    def test_requires_exact_clean_initialized_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root, source = Path(directory) / "parent", Path(directory) / "source"
            root.mkdir()
            source.mkdir()
            base = initialize(root)
            expected = initialize(source)
            git(root, "-c", "protocol.file.allow=always", "submodule", "add", str(source), "plugins")
            head = commit(root)
            report = ci_local.inspect_source(root, "candidate", base, head)
            self.assertEqual(report["gitlinks"], [{"path":"plugins", "expected":expected,
                "index":expected, "actual":expected, "initialized":True, "dirty":False}])
            self.assertEqual(lanes(report), ci_local.ci.NATIVE | {"services"})  # .gitmodules is shared configuration.
            module = root / "plugins"
            git(root, "config", "submodule.plugins.ignore", "all")
            write(module, "new.txt")
            report = ci_local.inspect_source(root, "worktree")
            self.assertIn("plugins", report["paths"])
            self.assertTrue(report["gitlinks"][0]["dirty"])
            self.assertEqual(lanes(report), ci_local.ci.NATIVE)
            with self.assertRaisesRegex(ValueError, "clean checkout"):
                ci_local.inspect_source(root, "candidate", base, head)
            git(module, "config", "user.name", "Fixture")
            git(module, "config", "user.email", "fixture@example.invalid")
            actual = commit(module)
            report = ci_local.inspect_source(root, "worktree")
            self.assertEqual(report["gitlinks"][0]["actual"], actual)
            self.assertNotEqual(actual, expected)
            with self.assertRaises(ValueError):
                ci_local.inspect_source(root, "candidate", base, head)
            git(module, "checkout", "--detach", expected)
            git(root, "submodule", "deinit", "-f", "plugins")
            with self.assertRaisesRegex(ValueError, "initialized"):
                ci_local.inspect_source(root, "candidate", base, head)


if __name__ == "__main__":
    unittest.main()
