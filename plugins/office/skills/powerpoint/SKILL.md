---
name: powerpoint
metadata:
  display-name: PowerPoint
description: Create and edit PowerPoint presentations (.pptx), themes, slide layouts, typography, speaker notes, tables, shapes and editable charts.
---

# Powerpoint

## Execution

Load `get_office_runtime` to obtain the execution Node's bundled Python executable and exact package versions. Use that executable with the existing `run_command` tool to run a script saved in the current worktree. Quote the executable and file paths. The runtime is already packaged: do not install into system Python, download another runtime, or change the bundled packages. Use the complete library APIs for both generation and editing. Inspect installed APIs with Python `help()` or `inspect` when needed; do not guess unavailable methods.

Native `read_office` and `export_pdf` provide inspection and PDF conversion. Author and edit with the script libraries. Scripts use the existing command tool's permissions and working directory. If command execution is disabled or denied, report that constraint without bypassing it.

For existing files, inspect the original and preserve unrelated content. For focused script edits, record the source hash, load the existing file, save to a sibling temporary file, reopen and validate it, recheck the source hash immediately before replacement, then use `os.replace`. Preserve the original and report a conflict if its hash changed. Do not rebuild an existing file from extracted plain text. A library may drop unsupported package parts: inspect those parts and use a focused ZIP/XML patch when necessary. Keep relationships, namespaces and content types consistent.

Before any save, create the destination directory with `Path(destination).parent.mkdir(parents=True, exist_ok=True)`. This applies to documents, images, exports, and temporary output files. Verify saved content and the requested changes. Use `export_pdf` for Office print previews, or the PDF libraries for PDF inspection/rendering. Conversion warnings are real limitations; successful export alone does not prove visual fidelity. Use the available image viewing tool to inspect representative rendered pages when layout matters. Keep editable source files. Return deliverables as ordinary standalone Markdown links, for example `[Report](output/report.docx)`. Sailry provides cards and side-panel previews; do not fabricate output paths or JSON cards.

## Presentation authoring

Use `python-pptx` (`from pptx import Presentation`) for editable text, shapes, images, tables, charts, slide dimensions, masters/layouts and speaker notes. Start from the supplied deck/template for edits. Keep slide bounds and consistent spacing; preserve unrelated elements. Use XML patches only for features missing from the high-level API, with package validation.

```python
from pptx import Presentation
from pptx.util import Inches, Pt
from pptx.chart.data import CategoryChartData
from pptx.enum.chart import XL_CHART_TYPE
prs = Presentation()
prs.slide_width, prs.slide_height = Inches(13.333), Inches(7.5)
slide = prs.slides.add_slide(prs.slide_layouts[6])
text = slide.shapes.add_textbox(Inches(0.6), Inches(0.4), Inches(12), Inches(0.8))
run = text.text_frame.paragraphs[0].add_run()
run.text, run.font.size = 'Quarterly results', Pt(32)
data = CategoryChartData()
data.categories = ['Q1', 'Q2', 'Q3']
data.add_series('Revenue', [100, 140, 180])
slide.shapes.add_chart(XL_CHART_TYPE.COLUMN_CLUSTERED, Inches(1), Inches(1.6), Inches(10), Inches(4.5), data)
slide.notes_slide.notes_text_frame.text = 'Explain the growth trend.'
prs.save('results.pptx')
```

Keep requested charts editable, not screenshots. Inspect source text runs before replacing them to avoid losing formatting. Check every slide's shapes, content, notes and chart series after saving. Render the deck and inspect overlaps/clipping; PDF preview is static and does not establish animation/video playback support.

## Package resources

- `templates/create.py` is an editable starter script, not a restricted authoring API. Read it with `read_skill_resource`, copy it into the worktree, then adapt its full library calls to the user's document. Run it with the shared Python executable and an output path. It creates missing parent directories and refuses to replace an existing file.
- `scripts/inspect_file.py` prints the saved file's content for verification. Run `<python> <skill-directory>/scripts/inspect_file.py <file>` using the absolute directory returned by `load_skill`. Quote paths and keep package resources unchanged. Structural inspection alone does not prove layout fidelity.

The plugin contains no interpreter. `get_office_runtime`, `read_office` and `export_pdf` are supplied by Sailry's Files capability; `run_command` uses Commands. If a required capability is unavailable, explain what must be enabled. Installing or enabling this skill does not grant execution permission.
