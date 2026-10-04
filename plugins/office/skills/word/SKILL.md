---
name: word
metadata:
  display-name: Word
description: Create and edit Word documents (.docx), templates, reports, typography, tables, headers, footers and comments, and export PDF.
---

# Word

## Execution

Load `get_office_runtime` to obtain the execution Node's bundled Python executable and exact package versions. Use that executable with the existing `run_command` tool to run a script saved in the current worktree. Quote the executable and file paths. The runtime is already packaged: do not install into system Python, download another runtime, or change the bundled packages. Use the complete library APIs for both generation and editing. Inspect installed APIs with Python `help()` or `inspect` when needed; do not guess unavailable methods.

Native `read_office` and `export_pdf` provide inspection and PDF conversion. Author and edit with the script libraries. Scripts use the existing command tool's permissions and working directory. If command execution is disabled or denied, report that constraint without bypassing it.

For existing files, inspect the original and preserve unrelated content. For focused script edits, record the source hash, load the existing file, save to a sibling temporary file, reopen and validate it, recheck the source hash immediately before replacement, then use `os.replace`. Preserve the original and report a conflict if its hash changed. Do not rebuild an existing file from extracted plain text. A library may drop unsupported package parts: inspect those parts and use a focused ZIP/XML patch when necessary. Keep relationships, namespaces and content types consistent.

Before any save, create the destination directory with `Path(destination).parent.mkdir(parents=True, exist_ok=True)`. This applies to documents, images, exports, and temporary output files. Verify saved content and the requested changes. Use `export_pdf` for Office print previews, or the PDF libraries for PDF inspection/rendering. Conversion warnings are real limitations; successful export alone does not prove visual fidelity. Use the available image viewing tool to inspect representative rendered pages when layout matters. Keep editable source files. Return deliverables as ordinary standalone Markdown links, for example `[Report](output/report.docx)`. Sailry provides cards and side-panel previews; do not fabricate output paths or JSON cards.

## Word authoring

Use `python-docx` (`from docx import Document`), with `docx.shared` units and `docx.oxml`/`lxml` for OOXML features outside its high-level API. Font families, sizes, paragraph alignment/spacing/indentation, section size/margins, headers/footers, tables, images and page breaks are available. Use OOXML for details outside the high-level API.

For Chinese documents, set both `run.font.name` and `w:rFonts/@w:eastAsia`; installing a font name in the XML does not install the font. Use fonts available on the execution Node and report substitutions that affect layout. Do not claim compliance with a named administrative-document standard without checking its actual requirements and the output.

```python
from docx import Document
from docx.shared import Mm, Pt, RGBColor
from docx.oxml import OxmlElement
from docx.oxml.ns import qn

doc = Document()
section = doc.sections[0]
section.page_width, section.page_height = Mm(210), Mm(297)
section.top_margin, section.bottom_margin = Mm(30), Mm(25)
section.left_margin = section.right_margin = Mm(28)
p = doc.add_paragraph()
p.paragraph_format.line_spacing = Pt(28)
p.paragraph_format.space_after = Pt(12)
run = p.add_run('Document heading')
run.font.name, run.font.size = 'Noto Sans CJK SC', Pt(22)
run._element.get_or_add_rPr().rFonts.set(qn('w:eastAsia'), 'Noto Sans CJK SC')
run.font.color.rgb = RGBColor.from_string('C00000')
borders = OxmlElement('w:pBdr')
line = OxmlElement('w:bottom')
for name, value in {'val':'single', 'sz':'12', 'space':'4', 'color':'C00000'}.items():
    line.set(qn('w:' + name), value)
borders.append(line)
p._p.get_or_add_pPr().append(borders)
footer = section.footer.paragraphs[0]
field = OxmlElement('w:fldSimple')
field.set(qn('w:instr'), 'PAGE')
footer._p.append(field)
doc.save('report.docx')
```

Use named styles for consistent hierarchy. Inspect all relevant sections, tables, headers and footers. Assigning `paragraph.text` removes run formatting; retain runs for focused edits. `Document.add_comment` supports comments in the pinned library; tracked revisions require careful OOXML handling, not merely changing text. Reopen the saved DOCX, inspect package XML for requested fields, and check pagination, borders and table continuation in the preview.

## Package resources

- `templates/create.py` is an editable starter script, not a restricted authoring API. Read it with `read_skill_resource`, copy it into the worktree, then adapt its full library calls to the user's document. Run it with the shared Python executable and an output path. It creates missing parent directories and refuses to replace an existing file.
- `scripts/inspect_file.py` prints the saved file's content for verification. Run `<python> <skill-directory>/scripts/inspect_file.py <file>` using the absolute directory returned by `load_skill`. Quote paths and keep package resources unchanged. Structural inspection alone does not prove layout fidelity.

The plugin contains no interpreter. `get_office_runtime`, `read_office` and `export_pdf` are supplied by Sailry's Files capability; `run_command` uses Commands. If a required capability is unavailable, explain what must be enabled. Installing or enabling this skill does not grant execution permission.
