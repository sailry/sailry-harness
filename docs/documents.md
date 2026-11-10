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

The native Documents presentation follows the accepted panel contract in
[panel.md](panel.md); live document HTML renders in the bridge-free sandboxed
WebView described there. B5 supplies text inspection and immutable document reads;
B11 separately owns PDF rendering for OCR
([models.md](models.md#embedding-and-ocr)). Other-platform runtime acceptance is
performed on its target operating system rather than inferred from local builds.
