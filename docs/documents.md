# Documents

`internal/office` extracts positioned text from bounded PDF, DOCX, PPTX and XLSX
bytes. It has no filesystem, Node, OCR or process access. PDF uses the pinned
`ledongthuc/pdf` reader and rejects encryption even when its password is empty.
XLSX uses pinned Excelize raw values and formulas after package and coordinate
validation. DOCX and PPTX use bounded ZIP/XML readers; presentation relationship
order and slide notes are retained. Text sections paginate in 128 KiB chunks,
with eight chunks per page and an immutable input revision.

Package inspection rejects duplicate parts, expansion beyond limits, malformed
XML, invalid worksheet coordinates and relationships escaping the package.
Workbook extraction includes explicit empty cells and preserves cached values,
formulas, sheet order and cell bounds without entering drawing or chart code.
The repository includes independent local fixtures for all five accepted formats
and malformed worksheet input.

Image inspection preserves supported small originals. Oversized or animated
inputs are bounded before decode, normalized to a still image, oriented when
transformed, resized to the model limit and encoded within the sent-byte budget.
Image facts distinguish source and transformed content. Animation checks inspect
container blocks without decoding every frame.

Session document metadata and immutable artifact revisions use the existing SQL
schema and confined profile files. Reads capture metadata, release their SQL
lease, verify exact stored HTML size and digest, then recheck conversation
ownership. Forked reads require the exact published artifact receipt in visible
canonical history; sharing a turn alone grants no artifact access. Artifact writes require the actual admitted native package owner, frozen turn
catalog, claimed approval and exact canonical tool arguments. Document fragments
use the pinned bluemonday policy plus bounded form, image and chart validation;
opaque previews retain authored source. Physical publication and SQL receipts are
separate, preserve immutable referenced revisions and can replace only an
unreferenced orphan. Package view resources retain their owning package reference.

A committed write publishes the `artifact` conversation change to the open
conversation after its receipt is stored (`recordWorkspaceEffect`), and a
conversation subscribed later receives the first `list_artifacts` page (64) in
its snapshot's `artifacts`, as the reference does. The desktop rereads the
session's documents when either changes.

Removing a session deletes its artifacts in the same transaction unless a
remaining conversation still shows a published revision, as a fork does. The
artifact directory of a removed session that keeps no artifact is deleted after
the commit; a directory that was not removed then goes with the next removal
([node.md](node.md#current-commands)).

The native Documents presentation follows the accepted panel contract in
[panel.md](panel.md); live document HTML renders in the bridge-free sandboxed
WebView described there. Because that view has no bridge, a document's form
(the `brief` package's `<form class="brief-form">`, stored and validated as
above) is shown and cannot be answered: the reference mounts the package's
`artifact_view`, keeps each form's draft in the `artifact_drafts` preference,
refuses a submit for a revision that is no longer the latest and sends the
answers as a user message to the document's session. Wiring it needs
`read_artifact` to return the package's view, a posting bridge in the document
view and the draft, stale and submit handling; the preference field and the
`artifact_form_*` strings of `locales/artifact.*.yml` stay for it. B5 supplies text inspection and immutable document reads;
B11 separately owns PDF rendering for OCR
([models.md](models.md#embedding-and-ocr)). Other-platform runtime acceptance is
performed on its target operating system rather than inferred from local builds.
