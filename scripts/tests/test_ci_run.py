"""CI progress uses native process evidence without changing child results."""

import importlib.util
import io
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import Mock, call, patch


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("ci_run", ROOT / "scripts/ci-run.py")
ci_run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci_run)


class Snapshots(unittest.TestCase):
    def test_windows_uses_native_process_evidence(self):
        snapshot = "PID PPID ELAPSED(s) CPU(s) RSS(KiB) COMMAND\n42 10 120 30.5 2048 rustc.exe\n"
        with patch.object(ci_run.sys, "platform", "win32"), \
                patch.object(ci_run.subprocess, "run", return_value=Mock(stdout=snapshot)) as run, \
                patch("sys.stdout", new_callable=io.StringIO) as output:
            ci_run.progress(60)
        command = run.call_args.args[0]
        self.assertEqual(command[:5], ["powershell.exe", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        self.assertIn("$ErrorActionPreference = 'Stop'", command[5])
        self.assertIn("Get-CimInstance -ClassName Win32_Process", command[5])
        for name in ("cargo.exe", "cargo-nextest.exe", "rustc.exe", "sccache.exe", "cl.exe", "link.exe"):
            self.assertIn(f"'{name}'", command[5])
        self.assertEqual(run.call_args.kwargs, {
            "capture_output": True, "text": True, "encoding": "utf-8", "errors": "replace",
            "timeout": 10, "check": True,
        })
        self.assertEqual(output.getvalue(), (
            "CI phase still running after 60s\n"
            "RSS is KiB; CPU time is cumulative seconds:\n" + snapshot
        ))

    def test_windows_reports_no_matching_processes(self):
        with patch.object(ci_run.sys, "platform", "win32"), \
                patch.object(ci_run.subprocess, "run", return_value=Mock(stdout="\r\n")), \
                patch("sys.stdout", new_callable=io.StringIO) as output:
            ci_run.progress(60)
        self.assertIn("No build processes found\n", output.getvalue())
        self.assertNotIn("unavailable", output.getvalue())

    def test_unix_retains_selected_processes(self):
        header = "PID PPID ELAPSED TIME %CPU RSS COMM"
        cargo = "42 10 02:00 00:30 25.0 2048 /opt/rust/bin/cargo"
        rustc = "43 42 01:00 00:10 16.7 1024 /opt/rust/bin/rustc"
        snapshot = f"{header}\n{cargo}\n20 1 03:00 00:00 0.0 512 /usr/bin/unrelated\n{rustc}\n"
        for platform in ("darwin", "linux"):
            with self.subTest(platform=platform), patch.object(ci_run.sys, "platform", platform), \
                    patch.object(ci_run.subprocess, "run", return_value=Mock(stdout=snapshot)) as run, \
                    patch("sys.stdout", new_callable=io.StringIO) as output:
                ci_run.progress(60)
            run.assert_called_once_with(
                ["ps", "-axo", "pid,ppid,etime,time,pcpu,rss,comm"],
                capture_output=True, text=True, timeout=10, check=True,
            )
            self.assertEqual(output.getvalue(), (
                "CI phase still running after 60s\n"
                f"RSS is KiB; CPU time is cumulative:\n{header}\n{cargo}\n{rustc}\n"
            ))

    def test_reports_failure_causes(self):
        cases = (
            (FileNotFoundError(2, "Executable not found", "powershell.exe"), "Executable not found"),
            (subprocess.CalledProcessError(1, ["powershell.exe"], stderr="CIM access denied\n"),
             "CIM access denied"),
            (subprocess.CalledProcessError(1, ["powershell.exe"], output="CIM query failed\n"),
             "CIM query failed"),
            (subprocess.TimeoutExpired(["powershell.exe"], 10, stderr=b"CIM request stalled\n"),
             "CIM request stalled"),
        )
        for error, cause in cases:
            with self.subTest(error=error), patch.object(ci_run.sys, "platform", "win32"), \
                    patch.object(ci_run.subprocess, "run", side_effect=error), \
                    patch("sys.stdout", new_callable=io.StringIO) as output:
                ci_run.progress(60)
            self.assertIn(f"Process snapshot unavailable: {error}\n", output.getvalue())
            self.assertIn(cause, output.getvalue())


class CommandResults(unittest.TestCase):
    def test_preserves_exit_status_after_snapshot_failure(self):
        command = ["fixture.exe", "argument with spaces", '--value="quoted"']
        environment = {"PATH": "fixture path"}
        for code, expected in ((0, 0), (23, 23), (-15, 143), (0xC0000005, 0xC0000005)):
            process = Mock()
            process.wait.side_effect = [subprocess.TimeoutExpired(command, 60), code]
            process.__enter__ = Mock(return_value=process)
            process.__exit__ = Mock(return_value=False)
            with self.subTest(code=code), patch.object(ci_run.sys, "platform", "win32"), \
                    patch.object(ci_run.subprocess, "Popen", return_value=process) as spawn, \
                    patch.object(ci_run.subprocess, "run", side_effect=FileNotFoundError("powershell.exe")), \
                    patch.object(ci_run.time, "monotonic", side_effect=[100, 160, 161]), \
                    patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": ""}), \
                    patch("sys.stdout", new_callable=io.StringIO) as output:
                self.assertEqual(ci_run.run(command, environment), expected)
            spawn.assert_called_once_with(command, env=environment)
            self.assertEqual(process.wait.call_args_list, [call(timeout=60), call(timeout=60)])
            self.assertIn("Process snapshot unavailable:", output.getvalue())
            self.assertIn(f"CI phase finished in 61.0s (exit {code})\n", output.getvalue())


if __name__ == "__main__":
    unittest.main()
