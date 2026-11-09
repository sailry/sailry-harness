# Conversation exports

Client owns selection and hydration of committed conversation history. Desktop
owns local offline rendering and destination publication. Exports use the same
authenticated Node reads as the transcript and never read a second history store.

`Client.ExportConversation` exports all committed history. `Client.ExportReply`
exports the selected turn's user input and final answer. Both follow logical
history pages and complete partial selected turns using revision-checked turn
continuations. Repeated cursors, changed canonical identities and history
revision conflicts fail the export. The selection is bounded to 64 MiB. Context
cancellation stops pending reads and rendering.

Unsent queue items, streaming drafts, automatic notice inputs, reasoning and work
before the final answer boundary are excluded. Closing questions and plan reviews
do not hide the final answer preceding them. Adjacent text fragments stay joined,
so Markdown syntax can span transport parts. Citation links use HTTP or HTTPS
without embedded credentials. Raw HTML is escaped and script URL destinations
are disabled. User-authored text is retained literally in Markdown.

Attachment metadata and resolved conversation image references accompany each
export document. Asset names combine attachment identity with a sanitized name.
A repeated identity with changed metadata fails. A reference to a tool image
resolves only within the exported conversation. Unknown image references remain
unchanged. Node-owned attachment download supplies the bytes; exports grant no
extra filesystem or tool access.

`internal/desktop/exporter.Render` returns Markdown, standalone HTML or PNG bytes.
The application owns the save panel, authenticated asset acquisition, destination
conflict handling, atomic publication and outcome toast. Markdown uses attachment
sidecars and a Sailry footer. HTML and PNG embed assets and use the same branding.

MathJax, its SVG fonts, Mermaid, ECharts and html2canvas are bundled with their
pinned provenance and licenses under `internal/desktop/exporter/assets`. All
rendering resources are local. A private native page has a restrictive content
policy, an ephemeral session and no callable Go bridge. Resource responses are
scoped to that exact page. Navigation, new windows and downloads are blocked by
MyGo's Untrusted boundary. A page-local result promise returns only the rendered
HTML or image tiles through host evaluation. Rendering has the source's 90-second
deadline. PNG is 960 pixels wide, capped at 32768 pixels high, and assembled from
2048-pixel tiles whose decoded dimensions are verified.

Native rendering supports macOS and Windows. Linux returns an unavailable fault,
matching the current product boundary. Compilation on another OS does not prove
its rendering behavior. Native acceptance must run on each supported system.

The pinned MyGo API cannot size an unplaced child WebView independently. The
exporter therefore uses a hidden 960-by-768 content-sized WebView window parented
to the existing application window. It closes after completion, cancellation or
parent closure and does not alter accepted application layout.

`TestConversationExport` groups final-answer selection, fragments, formulas,
HTML safety, attachment identity, image references, notice omission, partial-turn
hydration and cancellation fixtures. `TestExportRendering` groups offline asset
availability, page-local result transport, standalone output validation and PNG
tile assembly. These deterministic tests do not start native windows or contact
providers.
