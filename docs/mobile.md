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
| Speech operations may report unavailable until the speech phase | The adapter preserves operation names without adding another speech implementation |
| `watch_terminal`, `watch_login` and `watch_connector_login` deliver a single disconnected view with an `unavailable` error, and `watch_connections` fails with `unavailable` | The shared Client does not observe terminals, authorization attempts or plugin connections; the adapter preserves the names the Dart API calls |
