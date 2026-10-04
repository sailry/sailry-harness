"""Copy and adapt this starter; use the full library API for the requested document."""
import argparse
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('output', type=Path)
output = parser.parse_args().output
if output.exists():
    parser.error('destination already exists; choose a new path')
output.parent.mkdir(parents=True, exist_ok=True)

from reportlab.pdfgen import canvas
from reportlab.lib.pagesizes import A4
from pypdf import PdfReader

pdf = canvas.Canvas(str(output), pagesize=A4)
pdf.setTitle('Report')
pdf.setFont('Helvetica-Bold', 20)
pdf.drawString(54, 780, 'Report')
pdf.setFont('Helvetica', 11)
pdf.drawString(54, 750, 'Replace this content with the requested report.')
pdf.drawString(54, 725, 'Register a Unicode font for multilingual content.')
pdf.showPage()
pdf.save()
assert len(PdfReader(output).pages) == 1
