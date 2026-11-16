# Turso native runtime

Sailry uses the official `turso.tech/database/tursogo` 0.8.3 Go SDK and its
`database/sql` connector. This crate packages the official 0.8.3 local and sync
SDK ABIs with the native FTS index enabled. The SDK initializes both ABIs even
for local databases, so the local-only `turso_sdk_kit` library is insufficient.

The exact crate sources and dependencies are recorded in `Cargo.toml` and
`Cargo.lock`. Both SDK crates identify upstream commit
`607bff9fd0e547a25a97b3bf06e83bcc5d99d53c`. `fts` is enabled on the local SDK;
Cargo unifies it into the core used by both SDKs. The core includes MVCC; the
Go connector enables `experimental=index_method`, and Store verifies the
database journal mode. Database files are opened in place by Turso.

Three small functions in `sailry_turso.h` open an official native connection,
release its database owner, and call the official Rust `interrupt()` method.
The Go connector passes that connection to official `sdk.NewConnection` for
all SQL execution. This extension is needed because SDK 0.8.3 does not export
interruption through its C ABI, and its context checks alone cannot stop a
CPU-bound statement. Cancellation callbacks must finish before the connection
is closed or returned to its pool.

Run `sh native/turso/build.sh` on the target platform with the standard rustup
`cargo`/`rustc` proxies first in `PATH` (`$HOME/.cargo/bin`). They read the
pinned `rust-toolchain.toml`; a custom Cargo executable can use a different
output layout. The pinned toolchain produces one file in
`.runtime/native/<goos>-<goarch>/`:

| Target | Filename |
| --- | --- |
| macOS | `libturso_sync_sdk_kit.dylib` |
| Linux | `libturso_sync_sdk_kit.so` |
| Windows | `turso_sync_sdk_kit.dll` |

Linux release builds set `SAILRY_GLIBC` to the Host installer's supported floor,
as for iroh. The native CI job supplies this file and its `SHA256SUMS` to
`go run ./native/fetch -artifacts <dir>`. The fetcher includes it in both Host
and Desktop bundles. Seal the completed package after signing with
`go run ./native/fetch -seal <path>` (and `-host` for Host).

Release builds load only the library in the shipped directory after validating
its target, size and SHA-256 against `native-seal.json`. Source builds can use
the containing checkout's staging directory or set `SAILRY_TURSO_LIB` to a
specific file with the platform filename. They validate the file against a
seal when one is present. An unsealed source override is a developer input.

The official SDK has no path option. `internal/turso/loader.go` resolves the
selected file before initialization, then temporarily restricts the platform's
loader search variable to its directory while calling official `InitLibrary`
with the system strategy. The environment is restored immediately, including
on failure. Initialization runs once before opening database connections; the
search variable is process-wide during that call. The embedded stock SDK
library is never selected, because it does not establish FTS support.

## Existing local validation artifact

One previously built macOS arm64 library is available locally:

- Size: 20,023,976 bytes.
- SHA-256: `03883e3c3212628fe4083f1877e4ec1bea328d0435ee4743aab3d85e15357423`.
- Upstream version and commit in its Cargo `built.rs`: 0.8.3 and
  `607bff9fd0e547a25a97b3bf06e83bcc5d99d53c`.
- Its Cargo fingerprint includes the core `fts` feature, and its exported
  symbols include both official SDK ABIs.

That artifact established the original SDK and FTS behavior, but does not have
the three interruption/ownership extension symbols. It cannot run the current
connector. Build this crate once for each target, then reuse the resulting
artifacts while its source, dependency lock, build configuration and target
remain unchanged. Do not substitute a stock platform-library binary merely
because its filename matches.
