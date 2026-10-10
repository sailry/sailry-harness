import importlib.metadata
import os
from pathlib import Path
import shlex
import stat
import sys

root = Path(sys.prefix)
destination = Path(sys.argv[1])
scripts = root / ("Scripts" if os.name == "nt" else "bin")
python = destination / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
dispatcher = root / ".sailry-entry.py"
dispatcher.write_text(
    "import importlib.metadata, sys\n"
    "name = sys.argv.pop(1)\n"
    "entry = next(entry for distribution in importlib.metadata.distributions() "
    "for entry in distribution.entry_points "
    "if entry.group == 'console_scripts' and entry.name == name)\n"
    "sys.exit(entry.load()())\n",
    encoding="utf-8",
)
def writable(path):
    if path.is_symlink():
        raise ValueError("staged launcher cannot be a symbolic link")
    if path.exists():
        if not path.is_file():
            raise ValueError("staged launcher must be a regular file")
        path.chmod(path.stat().st_mode | stat.S_IWUSR)


entries = {
    entry.name
    for distribution in importlib.metadata.distributions()
    for entry in distribution.entry_points
    if entry.group == "console_scripts"
}
for name in sorted(entries):
    if not name or any(character in name for character in '\\/\r\n"%'):
        raise ValueError("console script name cannot be represented")
    final_dispatcher = destination / dispatcher.name
    if os.name == "nt":
        values = [str(python), "-B", str(final_dispatcher), name]
        if any('"' in value or '\n' in value or '\r' in value for value in values):
            raise ValueError("console script path cannot be represented")
        quote = lambda value: '"' + value.replace("%", "%%") + '"'
        path = scripts / (name + ".cmd")
        writable(path)
        path.write_text(
            "@echo off\r\n" + " ".join(quote(value) for value in values) + " %*\r\n",
            encoding="utf-8",
        )
        for suffix in [".exe", "-script.py"]:
            path = scripts / (name + suffix)
            writable(path)
            path.unlink(missing_ok=True)
    else:
        path = scripts / name
        writable(path)
        path.write_text(
            "#!/bin/sh\nexec "
            + " ".join(shlex.quote(str(value)) for value in [python, "-B", final_dispatcher, name])
            + ' "$@"\n',
            encoding="utf-8",
        )
        path.chmod(0o700)
# Activation files are optional convenience scripts; rewrite only the exact
# staging prefix rather than transforming installed package code or data.
for path in scripts.iterdir():
    if path.name.lower().startswith("activate") and path.is_file():
        writable(path)
        content = path.read_text(encoding="utf-8")
        path.write_text(content.replace(str(root), str(destination)), encoding="utf-8")
configuration = root / "pyvenv.cfg"
writable(configuration)
content = configuration.read_text(encoding="utf-8")
configuration.write_text(content.replace(str(root), str(destination)), encoding="utf-8")
