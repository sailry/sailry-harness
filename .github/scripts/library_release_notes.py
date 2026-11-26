"""Extract release notes for a canonical Go module version tag."""

import re
import sys
from pathlib import Path


def release_notes(tag, changelog):
    number = r"(?:0|[1-9][0-9]*)"
    identifier = rf"(?:{number}|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
    if not re.fullmatch(rf"v{number}\.{number}\.{number}(?:-{identifier}(?:\.{identifier})*)?", tag):
        raise ValueError(f"Not a canonical Go version tag: {tag}")
    heading = f"## [{tag[1:]}]("
    lines = changelog.splitlines()
    for index, line in enumerate(lines):
        if line.startswith(heading):
            end = next((n for n in range(index + 1, len(lines)) if lines[n].startswith("## ")), len(lines))
            notes = "\n".join(lines[index + 1:end]).strip()
            if notes:
                return notes
            break
    raise ValueError(f"CHANGELOG.md has no release notes for {tag}")


if __name__ == "__main__":
    try:
        print(release_notes(sys.argv[1], Path("CHANGELOG.md").read_text(encoding="utf-8")))
    except ValueError as error:
        sys.exit(str(error))
