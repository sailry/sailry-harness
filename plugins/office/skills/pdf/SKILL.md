---
name: pdf
metadata:
  display-name: PDF
description: Read, generate, render and edit PDF documents, merge/split/rotate pages and fill form fields.
---

# Pdf

## Execution

Load `get_office_runtime` to obtain the execution Node's bundled Python executable and exact package versions. Use that executable with the existing `run_command` tool to run a script saved in the current worktree. Quote the executable and file paths. The runtime is already packaged: do not install into system Python, download another runtime, or change the bundled packages. Use the complete library APIs for both generation and editing. Inspect installed APIs with Python `help()` or `inspect` when needed; do not guess unavailable methods.

Native `read_office` and `export_pdf` provide inspection and PDF conversion. Author and edit with the script libraries. Scripts use the existing command tool's permissions and working directory. If command execution is disabled or denied, report that constraint without bypassing it.

For existing files, inspect the original and preserve unrelated content. For focused script edits, record the source hash, load the existing file, save to a sibling temporary file, reopen and validate it, recheck the source hash immediately before replacement, then use `os.replace`. Preserve the original and report a conflict if its hash changed. Do not rebuild an existing file from extracted plain text. A library may drop unsupported package parts: inspect those parts and use a focused ZIP/XML patch when necessary. Keep relationships, namespaces and content types consistent.

Before any save, create the destination directory with `Path(destination).parent.mkdir(parents=True, exist_ok=True)`. This applies to documents, images, exports, and temporary output files. Verify saved content and the requested changes. Use `export_pdf` for Office print previews, or the PDF libraries for PDF inspection/rendering. Conversion warnings are real limitations; successful export alone does not prove visual fidelity. Use the available image viewing tool to inspect representative rendered pages when layout matters. Keep editable source files. Return deliverables as ordinary standalone Markdown links, for example `[Report](output/report.docx)`. Sailry provides cards and side-panel previews; do not fabricate output paths or JSON cards.

## PDF authoring

Use `reportlab` to generate PDFs with precise layout and fonts, `pypdf` for page operations, metadata and forms, and `pdfplumber` for text/table extraction. `pypdfium2` (bundled with pdfplumber) renders pages for inspection. Keep the editable Word/Excel/PPT source when text must reflow; regenerate its PDF after changes.

```python
from reportlab.pdfgen import canvas
from reportlab.lib.pagesizes import A4
from pypdf import PdfReader
import pypdfium2 as pdfium
c = canvas.Canvas('report.pdf', pagesize=A4)
c.setFont('Helvetica', 18)
c.drawString(54, 780, 'Report')
c.save()
assert len(PdfReader('report.pdf').pages) == 1
pdf = pdfium.PdfDocument('report.pdf')
pdf[0].render(scale=1.5).to_pil().save('report-page-1.png')
```

Register an available Unicode font for multilingual documents; base PDF fonts do not cover arbitrary text. Inspect actual form field names/types before filling and reopen the saved values. A white rectangle or deleted extracted text is not secure redaction. These libraries are not OCR engines; scanned pages require a separate available OCR capability. Do not claim an empty extraction means a blank page. Verify page order/rotation, fonts, form values and rendered layout before delivering.

## Package resources

- `templates/create.py` is an editable starter script, not a restricted authoring API. Read it with `read_skill_resource`, copy it into the worktree, then adapt its full library calls to the user's document. Run it with the shared Python executable and an output path. It creates missing parent directories and refuses to replace an existing file.
- `scripts/inspect_file.py` prints the saved file's content for verification. Run `<python> <skill-directory>/scripts/inspect_file.py <file>` using the absolute directory returned by `load_skill`. Quote paths and keep package resources unchanged. Structural inspection alone does not prove layout fidelity.

The plugin contains no interpreter. `get_office_runtime`, `read_office` and `export_pdf` are supplied by Sailry's Files capability; `run_command` uses Commands. If a required capability is unavailable, explain what must be enabled. Installing or enabling this skill does not grant execution permission.
