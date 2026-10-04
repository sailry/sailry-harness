# Document preview distributions

The file sidebar embeds unmodified upstream browser distributions in
`apps/desktop/src/content/files/preview/web/viewers.zip`:

| Package | Version | Source | License |
| --- | --- | --- | --- |
| pdfjs-dist | 6.3.289 | https://github.com/mozilla/pdf.js | Apache-2.0 |

The archive retains each package's LICENSE/NOTICE files, including PDF.js font,
CMap and WebAssembly notices. `dependencies.json` pins the npm tarball versions
and SHA-512 integrity values. `python3 vendor.py` in that directory reproduces the
archive without executing npm lifecycle scripts. Runtime previews are offline;
no CDN, Node.js installation or browser-side access to project paths is needed.

The application-owned viewer files compose these distributions rather than
copying ChatGPT application source. The existing native WebView mounting adapter
continues to handle GPUI clipping, overlays, focus and unmounting.
