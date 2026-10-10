#!/usr/bin/env python3
"""Build a public plugin-view fixture for Flutter's native integration test."""
import base64
import json
from pathlib import Path
import sys


def main():
    root = Path(__file__).resolve().parents[4]
    package = root / 'plugins' / 'brief'
    manifest = json.loads((package / 'plugin.json').read_text())
    view = manifest['extensions']['dev.sailry.platform']['artifact_view']
    content = {
        'template': (package / view['template']).read_text(),
        'styles': [(package / path).read_text() for path in view['styles']],
        'scripts': [(package / path).read_text() for path in view['scripts']],
        'libraries': view['libraries'],
    }
    encoded = base64.b64encode(json.dumps(content).encode()).decode()
    Path(sys.argv[1]).write_text(json.dumps({'SAILRY_ARTIFACT_VIEW': encoded}))


if __name__ == '__main__':
    main()
