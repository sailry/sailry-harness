"""Management and CI arguments remain literal and preserve admission identity."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class HostTasks(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="sailry-task-fixture-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.home = self.root / "private home"
        self.binary = self.home / ".local/lib/sailry/sailry-host"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_text(
            f"#!{sys.executable}\n"
            "import json, sys\n"
            "print(json.dumps({'args':sys.argv[1:], 'input':sys.stdin.read()}))\n"
            "raise SystemExit(4)\n"
        )
        self.binary.chmod(0o755)
        self.wrapper = self.binary.with_name("sailry")
        shutil.copyfile(ROOT / "scripts/host-command.sh", self.wrapper)
        self.environment = {key: value for key, value in os.environ.items()
                            if not key.startswith("SAILRY_")}
        self.environment["HOME"] = str(self.home)

    def command(self, script, *arguments, text="", environment=None):
        path = self.wrapper if script == "host-command.sh" else ROOT / "scripts" / script
        return subprocess.run(["bash", str(path), *arguments],
                              cwd=self.root, env=environment or self.environment,
                              input=text, capture_output=True, text=True, timeout=10)

    def test_wrapper_resolves_installed_links(self):
        program = self.root / "system program"
        self.binary.parent.rename(program)
        command = self.root / "bin/sailry"
        command.parent.mkdir()
        for target in (program / "sailry", Path("../system program/sailry")):
            with self.subTest(target=target):
                command.symlink_to(target)
                output = subprocess.run(["bash", str(command), "submit", "--session", "session"],
                                        cwd=self.root, env=self.environment, input="",
                                        capture_output=True, text=True, timeout=10)
                self.assertEqual(output.returncode, 4, output.stderr)
                self.assertEqual(json.loads(output.stdout)["args"],
                                 ["submit", "--data-dir", str(self.home / ".sailry-host"),
                                  "--session", "session"])
                command.unlink()

    def test_wrapper_preserves_profiles_and_literal_values(self):
        text = "$(touch injected) `touch altered` spaced 中文 🙂"
        arguments = ["--session", "session", "--prompt", text, "--request-id", "request"]
        output = self.command("host-command.sh", "submit", *arguments)
        self.assertEqual(output.returncode, 4, output.stderr)
        self.assertEqual(json.loads(output.stdout)["args"],
                         ["submit", "--data-dir", str(self.home / ".sailry-host"), *arguments])
        self.assertFalse((self.root / "injected").exists())
        self.assertFalse((self.root / "altered").exists())
        profile = str(self.root / "explicit profile")
        output = self.command("host-command.sh", "wait", "--session", "session", "--turn", "turn",
                              "--data-dir", profile)
        values = json.loads(output.stdout)["args"]
        self.assertEqual(values, ["wait", "--session", "session", "--turn", "turn", "--data-dir", profile])
        self.assertEqual(values.count("--data-dir"), 1)

    def test_preserves_prompt_flags_as_arguments(self):
        output = self.command("host-command.sh", "submit", "--session", "session", "--prompt", "--data-dir")
        self.assertEqual(json.loads(output.stdout)["args"],
                         ["submit", "--data-dir", str(self.home / ".sailry-host"),
                          "--session", "session", "--prompt", "--data-dir"])

    def test_ci_preserves_stdin_and_exit_code(self):
        environment = {**self.environment, "SAILRY_COMMAND":str(self.binary), "SAILRY_REQUEST_ID":"request",
                       "SAILRY_SESSION":"session", "SAILRY_EXPECTED_REVISION":"7",
                       "SAILRY_PROFILE":str(self.root / "profile space"), "SAILRY_TIMEOUT":"9"}
        text = "Literal $(touch injected) and `touch altered`\n中文 🙂"
        output = self.command("ci-task.sh", "--no-wait", text=text, environment=environment)
        self.assertEqual(output.returncode, 4, output.stderr)
        value = json.loads(output.stdout)
        self.assertEqual(value["args"], ["submit", "--request-id", "request", "--timeout", "9",
                         "--session", "session", "--expected-revision", "7", "--data-dir",
                         environment["SAILRY_PROFILE"], "--stdin", "--no-wait"])
        self.assertEqual(value["input"], text)
        self.assertFalse((self.root / "injected").exists())
        self.assertFalse((self.root / "altered").exists())

    def test_ci_forwards_config_and_file_paths(self):
        environment = {**self.environment, "SAILRY_COMMAND":str(self.binary), "SAILRY_REQUEST_ID":"request",
                       "SAILRY_WORKTREE":"worktree", "SAILRY_CONFIG":str(self.root / "config space.json"),
                       "SAILRY_PROMPT_FILE":str(self.root / "任务 prompt.txt")}
        output = self.command("ci-task.sh", environment=environment)
        self.assertEqual(output.returncode, 4, output.stderr)
        self.assertEqual(json.loads(output.stdout)["args"], ["submit", "--request-id", "request", "--timeout", "600",
                         "--worktree", "worktree", "--config", environment["SAILRY_CONFIG"],
                         "--prompt-file", environment["SAILRY_PROMPT_FILE"]])
        # Conflicting scope is forwarded for the canonical CLI to reject, never silently discarded.
        environment["SAILRY_EXPECTED_REVISION"] = "7"
        output = self.command("ci-task.sh", environment=environment)
        self.assertIn("--expected-revision", json.loads(output.stdout)["args"])

    def test_ci_requires_identity_and_one_target(self):
        for values in [
            {}, {"SAILRY_SESSION":"session"}, {"SAILRY_REQUEST_ID":"request"},
            {"SAILRY_REQUEST_ID":"request", "SAILRY_SESSION":"session", "SAILRY_WORKTREE":"worktree"},
        ]:
            with self.subTest(values=values):
                output = self.command("ci-task.sh", environment={**self.environment, "SAILRY_COMMAND":str(self.binary), **values})
                self.assertNotEqual(output.returncode, 0)
                self.assertEqual(output.stdout, "")


if __name__ == "__main__":
    unittest.main()
