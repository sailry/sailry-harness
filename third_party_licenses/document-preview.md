# Document preview distributions

The file sidebar embeds integrity-pinned upstream browser distributions in
`apps/desktop/src/content/files/preview/web/viewers.zip`:

| Package | Version | Source | License |
| --- | --- | --- | --- |
| pdfjs-dist | 6.3.289 | https://github.com/mozilla/pdf.js | Apache-2.0 |
| docx-preview | 0.4.1 | https://github.com/VolodymyrBaydalka/docxjs | MIT |
| jszip | 3.10.2 | https://github.com/Stuk/jszip | MIT |
| xlsx | 0.20.3 | https://cdn.sheetjs.com/ | Apache-2.0 |
| @jvmr/pptx-to-html | 1.1.2 | https://github.com/JVMR/pptx-to-html | MIT |

The archive retains each package's LICENSE/NOTICE files, including PDF.js font,
CMap and WebAssembly notices. `dependencies.json` pins tarball versions, sources
and SHA-512 integrity values. From the repository root, run
`bash scripts/python.sh apps/desktop/src/content/files/preview/web/vendor.py` to
reproduce the archive without executing npm lifecycle scripts. Runtime previews are offline;
no CDN, Node.js installation or browser-side access to project paths is needed.

The application-owned viewer files compose these distributions rather than
copying ChatGPT application source. The existing native WebView mounting adapter
continues to handle GPUI clipping, overlays, focus and unmounting.

The PPTX distribution has one checked patch in `vendor.py`: chart ticks use a
relative epsilon and a finite progression budget instead of the upstream fixed
epsilon, which can loop excessively for tiny chart domains. Tests cover ordinary,
tiny and invalid scales. All other upstream distributions and license notices
remain unchanged. Office preview is a read-only content view, not a print-layout
or PDF export engine; cached spreadsheet values are shown without formula execution.
