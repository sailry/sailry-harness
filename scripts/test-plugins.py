"""Run every package's deterministic Node.js test without a shell glob."""

from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]


def main():
    raise SystemExit(subprocess.run(
        [sys.executable, str(ROOT / "plugins/scripts/test.py"), *sys.argv[1:]], cwd=ROOT,
    ).returncode)


if __name__ == "__main__":
    main()
