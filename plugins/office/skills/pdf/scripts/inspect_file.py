"""Inspect saved content; this does not replace layout or feature-specific validation."""
import argparse
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('source', type=Path)
source = parser.parse_args().source

from pypdf import PdfReader

reader = PdfReader(source)
print(json.dumps({
    'pages': [page.extract_text() for page in reader.pages],
    'fields': reader.get_fields(),
}, ensure_ascii=False, default=str, indent=2))
