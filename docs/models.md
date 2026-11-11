# Local models

Local inference runs inside the Node on runtimes packaged beside the executable
(`Contents/Resources` in the macOS application bundle).
The registry, downloads and scheduler live in `internal/localmodels`; each model
kind has its own package and registers one `localmodels.Capability` through
`node.LocalModelCapabilities`. The Settings wiring is in
[settings.md](settings.md#b11-local-model-wiring).

## Decision

`internal/localmodels/decision` answers typed questions (`noul`, `choice`,
`score`) with one forward pass per question and no generated text. It is a port
of the Rust `decision` crate onto the llama.cpp C API (`b11756`) through purego.
Inference is CPU-only. There is no fallback runtime, model or GPU path.

### Backend wiring

- **Command.** `decide` is routed in `internal/node/node.go` to
  `dispatchDecision` in `internal/node/localmodels_decision.go`. It is not
  durable and stops with its caller or with the Node. It is reached by a
  controller command and by plugin scripts through `sailry/decisions`, which
  requires the `decisions.run` plugin action.
- **Order.** Request bounds, runtime availability, the saved Decision choice,
  model capture, then `localmodels.Execute` in the interactive lane. A disabled
  switch reports `not_configured` "local decision service is disabled"; no
  requested and no selected model reports `not_configured` "a decision model
  must be selected". A requested model overrides the selected one but not the
  switch. Nothing else is tried.
- **Faults.** Invalid model or request: `invalid_request`. Missing, mismatching
  or unsupported runtime: `unavailable`. Cancellation: `cancelled`. Other
  native failures: `internal`. Availability in the model catalog carries the
  same `unavailable` reason.
- **Runtime.** The only search location is the directory of the running
  executable, with symlinks resolved. `llama-runtime.json` lies in that
  directory, or in `Contents/Resources` of a macOS bundle. The embedded
  `llama-runtime.lock.json` pins the build and, per target, the file names,
  sizes and SHA-256 digests. Every file must be an ordinary file whose size
  and digest match the manifest, and the manifest must repeat the lock; on
  macOS the manifest alone carries size and digest because signing rewrites
  the images. Files are verified before any is opened and are opened by
  absolute path. A successful runtime stays loaded for the process; a failure
  is reported as `unavailable` and checked again on the next request.
- **CPU backend.** When the registry has no CPU device, the plugin is the
  target's single CPU image, or on targets with per-instruction-set images the
  locked variant with the highest `ggml_backend_score`. It is registered when
  the first model loads.
- **Model.** One asset named `model.gguf`. The GGUF v3 header, metadata and
  tensor table are validated in Go before native code reads the file. The
  `laya` layout reads one head column at each option marker of an encoder and
  keeps the trained head budgets; `openjev` reads the logits of single-letter
  option labels after the last token.
- **Prompt.** Control tokens are recognized only in text the template itself
  writes. Request values (question identity, type, instructions, state, option
  keys and descriptions) are tokenized as ordinary text.
- **Cancellation.** The request context is checked between questions, while a
  template renders and inside the native forward pass through the abort
  callback. One callback serves the process.
- **Privacy.** Prompts, states and answers are never logged. Native logging is
  discarded.

### Ownership

| Concern | Owner |
| --- | --- |
| GGUF validation, runtime verification and loading, templates, inference | `internal/localmodels/decision` |
| Capability registration, `decide` routing, fault codes | `internal/node/localmodels_decision.go` |
| Enabled switch, selected model, verified files, queueing, idle release | `internal/localmodels` registry and scheduler |
| Plugin permission for `decide` | `internal/node/store/plugin_authority.go` |
| Staging the runtime and writing `llama-runtime.json` | packaging |

### Tests

- Default: `TestDecisionUnavailable` (empty runtime directory, unsupported
  targets, mismatching manifest and files) and `TestDecisionContracts` (GGUF
  validator on synthetic files, layout detection and laya trimming, answer
  arithmetic, template isolation and bounds, request and runtime rules). They
  need no native library or model.
- `//go:build integration`: `TestDecisionNative` compares prompt tokens, head
  positions, answers and usage with the recorded output of the official
  `b11756` server for the pinned laya model and the synthetic openjev
  fixture, and covers cancellation inside a forward pass and control-token
  isolation. It reads `SAILRY_DECISION_RUNTIME`, `SAILRY_DECISION_FIXTURES`,
  `SAILRY_DECISION_JULIA` and `SAILRY_DECISION_OPENJEV`.

### Deviations

- **Template engine.** The `systemone` template is rendered by
  `github.com/nikolalohinski/gonja/v2` v2.5.1, not minijinja. gonja has no
  output formatter hook, so every request value is bracketed with two
  private-use characters while it passes through the template. The two
  characters are chosen per render so that they occur in neither the template
  nor the request; no request text is rejected. The writer removes the
  brackets and records the bytes between them as value ranges. Consequences:
  - Output that does not derive from a request value (template string
    constants, numbers, loop indices) counts as template text. minijinja
    treats every expression output as a value.
  - Filters, tests and comparisons see the brackets, so `length`, equality and
    `trim` on a request value differ from Jinja.
  - A template that slices or reorders a request value so that a bracket is
    lost fails with "systemone template could not be rendered". A template
    that removes both brackets on purpose (`state[1:-1]`) emits that value as
    template text. The template comes from the verified model file, which
    defines the whole prompt; request values cannot cause this.
  - gonja evaluates `a or b` and `a and b` to a boolean, not to an operand.
    Templates must use `a if a else b`. The pinned models do.
- **Template limits.** minijinja's strict undefined mode, fuel (100,000) and
  recursion limit (32) are approximated. What is enforced:
  - Template source: at most 128 KiB, by GGUF validation.
  - Language: `if`, `for`, `set`, `with` and `raw` only. `macro`, `include`,
    `import`, `from`, `extends`, `block`, `filter` and `autoescape` are
    rejected when the model loads, which removes template recursion, file
    access and output capture. The loader holds the one template in memory.
    Globals are `range`, `namespace`, `dict`, `joiner` and `cycler`.
  - Undefined: gonja's `StrictUndefined`.
  - Work: 100,000 steps per render. Each write, each executed control
    structure and each item produced by `range` costs one step. This is not
    minijinja's instruction count: expression evaluation is not charged, and a
    loop over an existing sequence whose body neither writes nor executes a
    statement is charged once, not per item.
  - Time: no timer. The request context is checked at every step and stops
    the render with `cancelled`.
  - Output: at most 256 KiB of prompt, counted without the brackets, and the
    prompt must be UTF-8.
  - Nesting: at most 150 open brackets and 150 nested control structures,
    checked when the model loads (minijinja's parser bound). gonja's parser
    recurses with the nesting of the source, and a deeper template would
    overflow the stack instead of failing. Such a template is rejected as
    "unsupported systemone template".
- **Lock location.** The lock is embedded from
  `internal/localmodels/decision/llama-runtime.lock.json`, a copy of the
  packaging lock; this repository has no `scripts/package`.
- **Admission.** `decide` returns its receipt first and reports faults through
  the completion, like other non-durable Node reads. The Rust Node resolved
  the decision before admitting.
- **Messages.** File errors carry Go's cause text without the path, and loader
  errors carry the operating system loader's text; the wording differs from
  the Rust build.
- **Callback failures.** The abort callback only reads the request context and
  cannot panic, so the Rust "cancellation callback panicked" path has no
  counterpart.
- **Other platforms.** Only darwin-arm64 ran against the native runtime.
  Linux, Windows and darwin-amd64 loading, glibc detection and CPU variant
  selection compile here; the integration test must run on those systems.

## Embedding and OCR

`internal/localmodels/onnx` binds the part of the onnxruntime 1.28.2 C API
that sessions on the CPU need, through purego. `internal/localmodels/embedding`
turns text into unit-length vectors and `internal/localmodels/ocr` recognizes
text in images with PaddleOCR detection, text-line orientation and recognition
models. They are ports of the Rust `model-assets` runtime and of the
`embedding` and `ocr` crates. There is no GPU path, no fallback runtime or
model and no download from these packages.

### Wiring a backend

- **Runtime.** The only search location is the directory of the running
  executable, with symlinks resolved. The library (`libonnxruntime.1.28.2.dylib`,
  `libonnxruntime.so.1.28.2` or `onnxruntime.dll` with
  `onnxruntime_providers_shared.dll`) must be an ordinary file whose size and
  SHA-256 match the pin in `onnx/lock.go`; it is verified before it is opened
  and opened by absolute path (`RTLD_NOW|RTLD_LOCAL`, or `LoadLibraryEx`
  restricted to that directory and System32). One library serves the process
  under a mutex and is never unloaded. Sessions run sequentially with at most
  four intra-op threads, one inter-op thread and spinning off.
- **Availability.** `node.LocalModelCapabilities` registers both kinds with
  `onnx.Available`, which checks that the pinned files exist beside the
  executable without hashing them. Without them the model catalog reports the
  kind unavailable and registration, installation and inference return the
  `unavailable` fault "ONNX Runtime file is unavailable: …". A library that
  fails verification reports `unavailable` when the first model loads.
- **Model files.** Every ONNX file is parsed for a graph and rejected when a
  tensor refers to external data, before native code reads it. File identity
  (size and digest) is the registry's concern.
- **Embedding.** `embedding.Load(definition, directory)` reads `tokenizer.json`,
  `config.json` and `model.onnx` of a `transformer` format. The window is 512
  tokens, special tokens and prefix included; longer input returns
  `embedding.ErrTooLong` and is never truncated. `Encode(input, texts)` applies
  the definition's query or document prefix, pools by CLS or mean and
  normalizes in double precision; `Offsets(text)` returns the byte range of
  every token. The tokenizer is a Go implementation of the `tokenizer.json`
  features the pinned default model uses: added tokens, a regular-expression
  split, byte-level pre-tokenization, BPE and template post-processing. An
  installation encodes "Model compatibility check" with both prefixes. The
  engine is exposed as a capability and an exported API only; Knowledge (B12)
  is its consumer.
- **OCR engine.** `ocr.Load(definition, directory)` reads `detector.onnx`,
  `recognizer.onnx`, `dictionary.txt` and, when the format prescribes it,
  `orientation.onnx`, validates their interfaces and runs each once on a blank
  image. `Recognize(image, cancelled)` accepts at most 40,000,000 pixels and
  16,384 pixels per side and polls `cancelled` between model runs.
- **Consumers.** `Node.inspectDocument` in
  `internal/node/localmodels_runtime_document.go` serves `read_office` for
  workspace paths (through `files.Inspect`) and for turn attachments
  (`inspectWorkspaceFiles`). A file named `.png`, `.jpg`, `.jpeg`, `.webp` or
  `.gif` is decoded (format recognized from its content) and read as one
  section named "Image". A PDF page whose extracted text is blank is
  rasterized and recognized; pages with text are untouched. All recognition
  runs through `localmodels.Execute` on the interactive lane, so the scheduler
  loads the model on demand and releases it after its idle period.
- **Settings.** The saved OCR choice is read for every request. A disabled
  switch reports `not_configured` "OCR is disabled" for an image and leaves a
  PDF as extracted. A selected model is used when it is installed; with no
  selection (Auto) the first installed model in catalog order is used, then
  any installed one. No installed model reports `not_configured` "an OCR model
  must be installed" for an image and leaves a PDF as extracted.
- **Faults.** Undecodable image or failed inference: `unavailable` "OCR failed:
  …". Image above the pixel limit, page without a positive finite size or a
  page count that differs from the text extraction: `invalid_request`.
  Cancellation: `cancelled` "OCR operation was cancelled". A full lane: `busy`.
- **PDF rendering.** `ocr.OpenPDF` rasterizes with PDFium compiled to
  WebAssembly (`github.com/klippa-app/go-pdfium` v1.21.4 on wazero, no cgo and
  no native library). A page is rendered on white at twice its point size,
  limited to 2000 pixels on its longer side. One PDFium instance is created by
  the first scanned document of a process and serves one document at a time.

### Ownership

| Concern | Owner |
| --- | --- |
| Runtime pin, verification, loading, sessions, ONNX file validation | `internal/localmodels/onnx` |
| Tokenizer, prefixes, pooling, normalization | `internal/localmodels/embedding` |
| Detection and recognition decode, image decoding, PDF rasterization | `internal/localmodels/ocr` |
| Capability registration, OCR model selection, faults, document parts | `internal/node/localmodels_runtime*.go` |
| Enabled switch, selected model, verified files, queueing, idle release | `internal/localmodels` registry and scheduler |
| Text extraction and section paging | `internal/office` |
| Staging the onnxruntime library beside the executable | packaging |

### Tests

- Default, no native library or model: `TestRuntimeFiles` and `TestValidate`
  (onnx); `TestEncode` and `TestLoadRejects` (tokenizer on a synthetic
  `tokenizer.json`); `TestVectors` and `TestLoadWithoutRuntime` (embedding);
  `TestProcessing` (CTC decode, detection post-processing, resize, crop,
  image decoding, unavailable runtime) and `TestRenderPDF` (a
  committed two-page PDF through PDFium) for OCR; `TestLocalModelRuntime` in
  `internal/node`.
- `//go:build integration`: `TestInference`, `TestReferenceTokens`,
  `TestGraniteReference`, `TestReferenceImages`, `TestReferencePDF`,
  `TestImageInspection` and `TestScannedInspection` compare with output
  recorded from the Rust crates. They read `SAILRY_ONNXRUNTIME_DIR`,
  `SAILRY_EMBEDDING_MODEL_DIR`, `SAILRY_OCR_MODEL_DIR` and
  `SAILRY_LOCALMODELS_FIXTURES`.

### Deviations

- **PDF rasterizer.** Pages are rasterized by PDFium, not by the reference's
  renderer. Scanned (image-only) pages produce the same text. Vector-only
  pages are not pixel-identical, so their recognized text can differ from the
  reference by isolated characters, such as one space.
- **Unclip.** The detected region is expanded analytically: each side of the
  minimum-area rectangle moves outward by `area × 1.5 / perimeter`, rounded to
  two decimals. The reference offsets the polygon with a clipping library and
  takes the minimum-area rectangle of the result, which is the same rectangle
  for a convex quadrilateral.
- **Selected OCR model.** A selected model that is not installed is not
  replaced: the request reports that no model is installed. The reference
  falls back to the catalog order in that case.
- **Embedding formats.** Only `transformer` models with the tokenizer features
  listed above load. `static` tables and WordPiece or Unigram tokenizers fail
  to load with "embedding model is invalid" or "embedding tokenizer failed";
  no other tokenizer is tried.
- **Threads and cancellation.** onnxruntime's own thread pool is used with
  the reference's thread counts; worker threads are not lowered in priority.
  Cancellation is observed between model runs, not inside one.
- **Startup verification.** Node startup accepts the model files their
  receipts cover and hashes the others in the background. Until its hash
  completes such a model appears as missing in the published catalog and is
  then announced by `local_models_changed`; `read_local_models` and every
  operation that needs the model (OCR, embedding, decisions) wait for the hash
  within the caller's cancellation instead of reporting it as not installed.
  The reference hashes them before startup completes.
- **Availability check.** Catalog availability checks that the runtime files
  exist; size and digest are verified when the first model loads.
- **Free space.** Before requesting a file, the download
  (`assets.Prepare`, shared with the speech models) compares the bytes still
  to receive with the free space of the model directory's volume and refuses
  with a storage error that names the file, the bytes needed and the bytes
  free; a partial file keeps its checkpoint. The check is per file, so a
  cached file is never counted, and a volume that does not report its free
  space refuses nothing. The reference has no such check and fails when the
  write does.
- **Image decoding.** Images are decoded by Go's `image` packages and
  `golang.org/x/image/webp`. JPEG pixels can differ from the reference decoder
  by one level; the recognized text of the fixtures is equal.
- **Other platforms.** Only darwin-arm64 ran against the native runtime.
  Linux, Windows and darwin-amd64 loading compiles here; the integration
  tests must run on those systems.
