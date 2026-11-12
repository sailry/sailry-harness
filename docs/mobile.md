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
identifiers for explicit recovery. Handle 0 is the root: `close` and `dispose`
on it are refused, and only `sailry_close` releases every handle. A closed
connection is no longer its peer's Client for notification commands.

The Node observation of a connection retries from the first attempt on
(250 ms to 5 s, [Node](node.md#observation)): a phone opened while it or the
Host is offline shows the failure and connects when the route returns, without
a manual refresh.

## Uncertain outcomes

The phone never sends a request twice. The adapter has two operations for a
prepared request, both answering `{"Ok": output}` or `{"Err": fault}`, with
`"released": true` beside a fault that ends the request:

| Operation | Client call | Use |
| --- | --- | --- |
| `execute` | `Client.Settle` | Sends the request once; a result that was not observed is looked up by request ID |
| `resolve` | `Client.Resolve` | "Check result": asks what became of a retained request and never sends it |

| Answer | Dart `CommandFailure` | The page |
| --- | --- | --- |
| stored output | none | continues as the success it is |
| stored fault | its code | reports the failure and drops the request |
| `outcome_unknown` (still running, or the Node cannot be asked) | `retained` | keeps the request and offers the check again |
| `unavailable`, released (the request never reached the Node; a non-durable request has no stored outcome) | `released`, `rejected` | drops the request; nothing was sent twice, the user may send again |
| `outcome_unknown`, released (the Node restarted while it ran, or the operation recorded that its effect could not be confirmed) | `released`, `rejected` | drops the request and says once that the result is unconfirmed |

`HostConnection.execute` and `HostConnection.check` map these. Every page that
keeps a request (turn and guidance drafts, session creation, history actions,
session configuration, checkpoint restore, file save and trash, Git mutations,
terminal launch) keeps it only while `CommandFailure.retained` holds and
resolves it through `check`. A status action is the one exception: tapping it
again is the user repeating that action, so it sends its retained request
again through `execute`, which the Node answers from its ledger when it already
ran.

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

Transfer chunks (digest `update`, upload `write`, download `next`) are at most
64 KiB and cross the boundary as one base64 string; the adapter still reads a
JSON number array.

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

### Build commands

Requirements: Flutter 3.44.7 (`apps/mobile/.flutter-version`), Go 1.27.3, rustup
with toolchain 1.98.0 and the Rust targets of the table below, the Android NDK
selected by Flutter (`flutter.ndkVersion`) or Xcode with the iOS SDK.

| Target | Go target | Rust target | Staged output under `.runtime/native/` |
| --- | --- | --- | --- |
| Android `arm64-v8a` | `android/arm64` | `aarch64-linux-android` | `android-jni/arm64-v8a/libsailry_{mobile,iroh}.so` |
| Android `armeabi-v7a` | `android/arm`, `GOARM=7` | `armv7-linux-androideabi` | `android-jni/armeabi-v7a/libsailry_{mobile,iroh}.so` |
| Android `x86_64` | `android/amd64` | `x86_64-linux-android` | `android-jni/x86_64/libsailry_{mobile,iroh}.so` |
| iOS device | `ios/arm64` | `aarch64-apple-ios` | `ios-arm64/libsailry_mobile.a`, `SailryIroh.framework` |
| iOS simulator, Apple silicon | `ios/arm64` | `aarch64-apple-ios-sim` | `ios-simulator-arm64/…` |
| iOS simulator, Intel | `ios/amd64` | `x86_64-apple-ios` | `ios-simulator-amd64/…` |

The Flutter build runs the adapter build itself; no separate step is required:

```sh
cd apps/mobile
flutter pub get --enforce-lockfile
flutter build apk --release --target-platform android-arm64
flutter build appbundle --release
flutter build ios --release
```

- Android: Gradle's `prepareSailryNative` task runs before `preBuild`. It calls
  `sh cmd/sailry-mobile/build.sh android <abi>` for each ABI of Flutter's
  `--target-platform` (all three without it) with the NDK and `minSdk` of the
  Gradle project, and `jniLibs` reads `.runtime/native/android-jni`. Both
  libraries are linked with 16 KiB page alignment.
- iOS: the Runner target's `Build Sailry Native` phase runs
  `apps/mobile/ios/build_native.sh`, which calls
  `sh cmd/sailry-mobile/build.sh ios <sdk> <arch>` for each architecture of the
  Xcode build and embeds `SailryIroh.framework`. `ios/Flutter/Sailry.xcconfig`
  force-loads `libsailry_mobile.a`, keeps the six exports and links the
  framework. The static archive is linked per slice, so no XCFramework is built.

The same script builds one slice directly. Android needs `ANDROID_NDK_HOME`;
`SAILRY_ANDROID_API` (default 24) and `IPHONEOS_DEPLOYMENT_TARGET` (default
15.0) are optional:

```sh
ANDROID_NDK_HOME=/path/to/ndk sh cmd/sailry-mobile/build.sh android arm64-v8a
sh cmd/sailry-mobile/build.sh ios iphoneos arm64
sh cmd/sailry-mobile/build.sh ios iphonesimulator arm64
```

Android release signing reads `ANDROID_KEYSTORE_PATH`, `ANDROID_STORE_PASSWORD`,
`ANDROID_KEY_ALIAS` and `ANDROID_KEY_PASSWORD` from the environment; without
them the release build uses the debug signer, and `SAILRY_ANDROID_RELEASE=1`
refuses to build. iOS signing uses the local Xcode team and profiles;
`flutter build ios --release --no-codesign` produces an unsigned application.

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
| Claiming an invitation that was closed or replaced, or that already serves four claimants, fails with a network fault instead of `permission_denied` | The sharing Link closes such pairing connections before reading them, so silent connections cannot occupy its accept workers; a wrong code against an open invitation still answers `permission_denied` |
| Speech operations may report unavailable until the speech phase | The adapter preserves operation names without adding another speech implementation |
| `watch_terminal`, `watch_login` and `watch_connector_login` deliver a single disconnected view with an `unavailable` error, and `watch_connections` fails with `unavailable` | The shared Client does not observe terminals, authorization attempts or plugin connections; the adapter preserves the names the Dart API calls. What each needs is listed under [Unbound observers](#unbound-observers) |
| "Check result" asks the Node about the request (`inspect_request`) instead of executing the same request again, and a request the Node can never answer is released | Rust re-executes the request ID: a request that never arrived runs when the user checks, minutes later, and one interrupted by a Node restart answers `outcome_unknown` forever |
| Transfer chunks are base64 strings instead of JSON number arrays | A number array is about four times the size of the bytes it carries |
| `sailry_cancel` has no Dart caller | `NativeBridge.invoke` does not return its request ID; an operation ends with its handle. The export is kept for the ABI and covered by `TestMobileSubscribeCancel` |

### Unbound observers

The Node serves all four topics and the Desktop observes them in
`internal/desktop/live`, which Mobile cannot import. `internal/client` has only
the resuming `Client.Subscribe`; each observer needs a projection there and a
case in `Client.Watch` (`internal/client/observer.go`). The adapter and the Dart
API already carry the names and handle kinds.

| Operation | Topic | Needed in `internal/client` | Rust source |
| --- | --- | --- | --- |
| `watch_terminal` | `terminal` (terminal ID) | A terminal projection: accept `terminal_snapshot` and `terminal_frame` updates for this Node and terminal, order them by info revision and sequence, validate the screen and ask for recovery on a gap; an observer that reconnects with the 250 ms to 5 s backoff and publishes `{snapshot, connected, recovering, error}` | `crates/client/src/terminal.rs`, `terminal/projection.rs` |
| `watch_login` | `provider_login` (attempt request ID) | An attempt projection: refuse an update of another attempt or with revision 0, ignore an older revision, refuse a changed equal revision or a change after completion; publish `{update, connected, error}`, reconnect on a transient loss and end once the attempt is no longer active. The argument is the attempt, from which the topic takes the ID | `crates/client/src/login.rs`, `login/observer.rs` |
| `watch_connector_login` | `connector_login` (attempt request ID) | The same projection over connector login updates | `crates/client/src/connectors.rs`, `login/observer.rs` |
| `watch_connections` | `connections` | An observer whose `next` returns each `connections_changed` payload (a full list each time) and ends on any other update or a failure. No projection | `crates/mobile-bridge/src/api/connectors.rs` |

## Known display gaps

The phone decodes every value below (the Go Client decodes peer data
tolerantly and Dart reads maps), but shows none of them; each is a display gap
to close with Mobile UI work, not a compatibility fault.

| Protocol value | Phone today |
| --- | --- |
| `ConversationQueue.pause_reason` (`user`, `stopped`, `interrupted`, `unrecorded`) | The queue shows paused or not; a queue paused by a stop or an interruption looks paused by the user |
| `ConversationBackgroundTask.hold` (`limit`, `stopped`, `interrupted`, `failed`, `other`) | A held background task shows as an unknown outcome |
| `ConversationContinuation.kind`, `limit`, `user` | A package-continued turn looks like any continuation |
| `FaultDetails` kind `continuation` (`reason`, `count`, `limit`) and its refusal reasons | A refused continuation shows the generic failure |
| `continue_turn` members `marker`, `limit`, `user`; `PluginExtension.continuation_markers` | Not sent and not read; the phone does not continue a turn with a marker |
| `ToolHandler.approval` | Not read |
| `FileBrowserLocation.medium`, location kind `applications` | Locations are listed without a kind or medium; the new kind still renders as a row |
| `FileEntry.modified_ms` | Not shown |
| `BackupSettings.keep` and `last`, `BackupOutcome`, `BackupRecord.skipped` | Not shown |
| `KnowledgeFailure`, `KnowledgeProgress.failed` and `failures` | Not shown |
| `count_feature`, `FeatureUse` | Not sent; the Node accepts feature counts from its own Desktop only |
| Resource part `output_limit` | Not drawn (the timeline's default branch) |
| Error code `context_overflow` | Shows the generic failure label |
| A released request that never reached the Node | Shows the existing "Host not connected" or "failed" label; there is no "not sent" copy |
