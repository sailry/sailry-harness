"""The installed ACP management entry preserves stdio and the selected profile."""

import json
import unittest
import test_host_tasks as fixtures


class AcpWrapper(unittest.TestCase):
    def test_preserves_stdio_and_profile(self):
        fixture = fixtures.HostTasks()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        text = '{"jsonrpc":"2.0","method":"initialize","params":{"literal":"$(touch injected) `touch altered` 中文 🙂"}}\n'
        profile = str(fixture.root / "profile space")
        for arguments, expected in [
            ([], ["acp", "--data-dir", str(fixture.home / ".sailry-host")]),
            (["--data-dir", profile], ["acp", "--data-dir", profile]),
        ]:
            with self.subTest(arguments=arguments):
                output = fixture.command("host-command.sh", "acp", *arguments, text=text)
                self.assertEqual(output.returncode, 4, output.stderr)
                value = json.loads(output.stdout)
                self.assertEqual(value["args"], expected)
                self.assertEqual(value["input"], text)
                self.assertFalse((fixture.root / "injected").exists())
                self.assertFalse((fixture.root / "altered").exists())


if __name__ == "__main__":
    unittest.main()
