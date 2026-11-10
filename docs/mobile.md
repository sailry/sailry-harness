# Mobile controller

`apps/mobile` preserves the Flutter controller and its assets. The Dart API names
used by the application are retained by `packages/sailry_bridge`. Its native
adapter calls `cmd/sailry-mobile`; `internal/mobile` owns handles and delegates
controller behavior to shared Client, Link and Protocol. Source provenance lives
with the imported application and the unchanged pairing service.

## Ownership

Mobile opens a controller profile containing its identity and paired peers.
It connects directly to the selected execution Node; Desktop is not a gateway.
The adapter never starts a Node, provider transport, agent loop, PTY, Git
execution or business database. Shared Client owns projections, history paging,
notices, status selection and transfer verification. Dart owns Flutter view
state and translates JSON models.

Controller, connection, observer, upload, download and forwarding handles are
opaque native integers. Closing a parent cancels pending operations and releases
its descendants. Closing observation never cancels an admitted command.
Retained pairing claims and uncertain command requests preserve their original
identifiers for explicit recovery.

## C ABI and callbacks

`cmd/sailry-mobile/sailry_mobile.h` defines six exports: `sailry_open`,
`sailry_invoke`, `sailry_subscribe`, `sailry_cancel`, `sailry_free` and
`sailry_close`. No Go pointer crosses this boundary. Dart registers an operation
before native invocation and supplies its request ID, so an immediate native
completion cannot race registration.

Asynchronous results and observation pulls arrive through one
`NativeCallable.listener`. Each result is a JSON success or fault envelope with
the original request ID. The synchronous `project_modes`, `select_mode` and
`session_link` rules return C-owned JSON through the invoke out parameters,
preserving the application's existing synchronous signatures without a Dart
copy of those rules.

Native copies input before returning. Dart frees every returned C buffer exactly
once in `finally`. Bridge shutdown cancels and joins operations, releases handles
and emits a final completion count. Dart drains that count before closing its
listener; the libraries stay loaded for process lifetime.

## Platform builds

`cmd/sailry-mobile/build.sh` uses the pinned Rust toolchain and target SDK. Android
builds the Go C ABI as c-shared and iroh as a shared library, staging both under
`.runtime/native/android-jni/<abi>/` for Gradle. Android loads iroh first and
initializes its platform certificate verifier with the application context.

iOS builds the Go C ABI as c-archive and stages iroh as an embedded
`SailryIroh.framework` for the selected device or simulator slice. These are the
documented cgo exceptions; ordinary Host and Desktop builds keep cgo disabled.
Artifacts, signing inputs and local target configuration never enter Git.

## UI state vs backend data

| Flutter state | Shared backend facts |
| --- | --- |
| Routes, selection, focus, expansion and unsent input | Node snapshots, conversation history and connection state |
| Native handle wrappers and listener lifecycle | Client observations, admitted outcomes and retained pairing identity |
| Transfer controls | Verified bytes, progress and forwarding from the captured Client |

## Validation

The coordinator runs the Go gate, mobile cgo vet, Flutter analysis and the
preserved Flutter contract tests. Actual Dart callbacks and PIN pairing use
isolated integration profiles. A macOS c-shared or simulator result establishes
only that tested environment; physical Android and iOS continuation require
their own runtime acceptance. Platform compilation is not a claim that a device
paired or continued a conversation.

## Deviations

| Deviation | Reason |
| --- | --- |
| The native implementation is Go with a C ABI instead of the preserved Rust bridge | Both Mobile platforms reuse the same shared Client and Link behavior |
| Root pure rules have an immediate result path | Their existing Dart signatures are synchronous; waiting on a listener would block the Dart isolate |
| Speech operations may report unavailable until the speech phase | The adapter preserves operation names without adding another speech implementation |
