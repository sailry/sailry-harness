"""Inspect saved content; this does not replace layout or feature-specific validation."""
import argparse
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('source', type=Path)
source = parser.parse_args().source

from openpyxl import load_workbook

workbook = load_workbook(source, read_only=True, data_only=False)
try:
    print(json.dumps({sheet.title: list(sheet.values) for sheet in workbook},
                     ensure_ascii=False, default=str, indent=2))
finally:
    workbook.close()
