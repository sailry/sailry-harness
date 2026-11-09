# Contributing

Read [ARCHITECTURE.md](ARCHITECTURE.md) and [AGENTS.md](AGENTS.md) before changing
code. Follow the [code of conduct](CODE_OF_CONDUCT.md). Report vulnerabilities
through [the private security reporting process](SECURITY.md), not public issues.

## Issues and pull requests

| Change | Issue first |
|---|---|
| Feature, behavior, UI, protocol, storage or architecture | Required. Wait for maintainer agreement |
| Bug fix | Preferred. An obvious fix may go straight to a pull request with reproduction steps |
| Typo, documentation, translation or other small fix | Not needed |

Use the issue templates, keep one cohesive change per pull request, reference the
issue with `Closes #<number>` and fill in the pull request template, including how
you verified the change. Maintainers may skip the issue for their own work.

## Native setup

Requirements:

- Rust 1.98.0, Git, Python 3.12 or newer, Zig 0.16.0 (for the Ghostty binding),
  `just`, and your platform's native build tools.
- macOS Desktop: Xcode Command Line Tools.
- Linux Desktop: Speech Dispatcher and libclang (`libspeechd-dev` and
  `libclang-dev` on Debian or Ubuntu). Host does not need these.
- Plugin JavaScript tests: Node.js 22 or newer.

Python recipes pick an available Python 3.12+ without changing `PATH`. Set
`SAILRY_PYTHON` to choose one explicitly; an unsupported choice fails instead of
falling back.

Use committed lockfiles and `--locked` for Rust commands. Do not replace pinned
dependencies with absolute local paths.

```sh
git clone --recurse-submodules <repository>   # or: git submodule update --init --recursive
cargo build --locked -p sailry-host
just desktop                                  # app bundle using ~/.sailry
just desktop /absolute/private/desktop-profile
just host /absolute/private/host-profile
just preview                                  # identified example data, no Node
```

- Desktop and Host default to the `~/.sailry` profile, the same one the installed
  app uses. Close the installed app before running `just desktop` against it.
  `SAILRY_DESKTOP_DATA_DIR` or a recipe argument selects another profile; the
  argument wins.
- A profile holds Node configuration, plaintext credentials, plugin packages and
  data, and attachments. See [storage versions](ARCHITECTURE.md#contract-and-storage-versions)
  for what startup does to existing storage.
- `just preview` cannot be combined with profile or relay options.
- Build profiles: `dev` and `test` leave workspace code unoptimized and optimize
  dependencies; release uses `opt-level = "z"`. GPUI layout hot paths stay at
  level 3 in both.
- Mobile: `just mobile-deps`, `just mobile [device]`, `just check-mobile` and
  `just test-mobile-contract`.

Official plugins (`plugins/`) and themes (`themes/`) are separate repositories:
commit there first, then commit the reviewed Gitlink here, and push the package
commit before the Gitlink. Each submodule's README and AGENTS files cover package
maintenance.

## Checks

```sh
just ci fast        # Selected source and formatting checks
just ci affected    # Add affected lint, tests and production checks
just ci pre-push    # Before a push or PR: everything changed since the origin/main merge base
just tools          # Compare local tool versions with CI
just check          # Rust formatting and Clippy with warnings denied
just test-source    # Public guides, publication boundaries and source fixtures
just test-plugins   # Deterministic package JavaScript tests
just test-backend   # Shared Rust tests and package runtime contracts
just test-ui        # Desktop logic and service-contract tests
```

- `just ci` checks changes outside `HEAD`: staged, unstaged, untracked and
  submodule changes. Use `--base <commit>` to include committed changes and `--plan`
  to print the selected commands without running them. For a committed candidate,
  use a clean worktree whose checkout matches the candidate:
  `just ci affected --candidate --base <base> --head <candidate>`.
- `just ci pre-push` compares against the merge base of `origin/main` (or
  `--target <branch>`). It always runs formatting and the source, name and locale
  checks, then Clippy and product checks for affected packages. It fails when
  nothing changed or when a pinned tool version differs from CI. Off macOS, it
  reports macOS-only Clippy as not run. Prose-only changes select only the source
  checks.
- Test recipes use CI's nextest profile. Install the pinned runner with
  `cargo install --locked cargo-nextest --version 0.9.143`.
- Node integration tests need `--features sailry-node-runtime/test-support`.
  Standard test commands skip provider backoff waits against fixture servers. For
  direct Cargo runs, set `SAILRY_MODEL_SKIP_BACKOFF=true` to get the same behavior.
- Node integration features share a few compiled suites. Register new test files
  in the right suite; the source check rejects missing or duplicate registrations.
- Pairing-service checks have their own
  [guide](services/pairing-relay/README.md#run-and-verify).

## Test conventions

- Put small Rust unit tests in nearby `#[cfg(test)]` modules. Put cross-module
  integration tests and large fixtures in focused test modules or the crate's
  `tests/` directory.
- Group tests by the unit or behavior under test, so each name only states the
  distinguishing scenario, for example
  `mod panel_visibility { fn preserves_manual_choice() }`. Do not add a group just
  to shorten names.
- The naming check rejects repeated owning-scope prefixes. It allows at most seven
  semantic words in function identifiers and twelve in string-based test labels.
  Acronyms count as one word; required Python `test_` prefixes do not count.
  Generated bindings and third-party source are excluded. Passing the check does
  not replace naming review.
- Cover success and meaningful failure paths for changed public behavior. A
  regression test must fail without its fix. Compare complete structured outcomes
  where stable; not panicking is not enough.
- Keep tests deterministic and self-contained: isolated temporary profiles,
  injected clocks or bounded waits, and cleanup of spawned workers. Never depend on
  user projects, private profiles or ignored output directories.
- Cover data mappings exhaustively in unit tests and use one representative
  integration workflow per distinct behavior. Keep plugin business tests in the
  owning package; core services use neutral contract fixtures.
- Changed shared behavior needs domain evidence through the paired remote Link
  client, not a mocked transport. Test conflicts, reconnect and original-request
  recovery where a command can cause side effects.
- The Node `transport` test target runs each command and subscription contract
  through both local and remote clients against one Node. Extend it when a change
  touches that boundary (receipts, completions, faults, request reuse,
  subscriptions, streams, cancellation, plugin calls). Keep local-only behavior on
  the local client, pairing and Link behavior on the remote client, and run both
  only where behavior intentionally differs.
- Live-service and device tests use `#[ignore = "requires …"]` with an opt-in
  command, and never report success when prerequisites are missing. Test
  credentials are synthetic, fixtures use loopback, and secrets are never logged.
- Use simulated-window, rendered-layout, mouse or keyboard checks only as
  temporary debugging aids, and remove them before committing. GPUI Kit owns
  framework control coverage. Test controller logic without mounting a window.
- Do not replace removed UI interaction tests with screenshot, pixel, OS
  interaction or end-to-end gates. Run real-service, device, OS and performance
  checks only when requested, and report their prerequisites and results
  separately.
- A compile pass, fixture or mocked test proves only its own scope. Do not report
  it as live, device or whole-product acceptance.

## Continuous integration

- [Checks](.github/workflows/ci.yml) runs only on explicit dispatch
  (`gh workflow run ci.yml --ref <branch-or-tag>`). Pushes and pull requests do not
  start it. Version publication dispatches it for the exact tagged commit when no
  full result exists.
- Every dispatch runs all lanes in full: Source, strict Rust lint, shared Node,
  Desktop (macOS and Windows), Host (macOS and Linux), Mobile with iOS, Mobile
  Android with Dart FFI contracts, and the pairing service. `Checks passed` requires
  every selected job to succeed; skipped or cancelled jobs do not count.
- The optional `reuse_run` input reuses successful jobs from an earlier run only
  when product inputs are unchanged and provenance is valid.
- Integration tests that fail are retried once, and the summary lists tests that
  passed only on retry. A retry pass is a flaky test to diagnose, not a fix. Unit
  tests are not retried. Hung tests fail after five 60-second periods.
- CI supplies no model credentials, production profiles or OS permissions, so
  live-service and device tests stay opt-in. The native decision lane is
  [manual](crates/decision/tests/README.md). Actions and toolchains are pinned.

## Release packages

- [The version workflow](.github/workflows/release-version.yml) runs on a pushed
  `v*` tag (or manual dispatch from `main` with a `tag` input). The tag must match
  every application's version and be an ancestor of `main`. Publication requires a
  successful full Checks run at that exact SHA.
- It starts the [Desktop](.github/workflows/release.yml) and
  [Host](.github/workflows/release-host.yml) builds, publishes the release, and
  starts [Mobile](.github/workflows/release-mobile.yml) and the
  [changelog](.github/workflows/changelog.yml). Publish jobs wait for the
  exact-source readiness gate.
- Desktop: ad-hoc macOS arm64 and x64 packages first, then
  [formal signing and notarization](.github/workflows/release-desktop-formal.yml)
  and a signed `update-v1.json`, followed by the update-feed workflow. Windows x64
  ships an unsigned portable archive and a per-user installer outside the update
  catalog. Windows ARM64 is not built.
- Host: macOS and Linux arm64/x64 archives. Linux targets glibc 2.34 through a
  checksum-pinned [sysroot](scripts/package/linux-sysroot.lock.json).
- Mobile: signed Android ARM64 APK and iOS IPA, with optional TestFlight upload.
  App Store Connect steps (export compliance, tester groups) remain manual.
- Release notes come from the version's `CHANGELOG.md` section, or from tag
  commits when it is missing. Preview SemVer versions go to the experience channel
  and do not replace Latest.
- Signing secrets: `APPLE_CERTIFICATE_P12`, `APPLE_CERTIFICATE_PASSWORD`,
  `APPLE_API_PRIVATE_KEY`, `SAILRY_UPDATE_SIGNING_KEY`, `ANDROID_KEYSTORE_BASE64`,
  `ANDROID_STORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`,
  `IOS_CERTIFICATE_P12`, `IOS_CERTIFICATE_PASSWORD`, `IOS_PROVISIONING_PROFILE`.
  Variables: `APPLE_SIGNING_IDENTITY`, `APPLE_TEAM_ID`, `APPLE_API_KEY_ID`,
  `APPLE_API_ISSUER_ID`, `ANDROID_SIGNING_SHA256`. Never commit these values.

Local packaging:

Linux cross-builds also need Zig, LLVM `llvm-ar` and `llvm-strip` (on macOS,
`brew install llvm@21`), and `cargo-zigbuild 0.23.4` installed under
`target/tooling`:

```sh
cargo install --locked cargo-zigbuild --version 0.23.4 --root target/tooling
rustup target add x86_64-unknown-linux-gnu
```

```sh
cargo build --locked --release -p sailry-desktop -p sailry-host
SAILRY_SIGNING=developer-id SAILRY_NOTARIZE=1 bash scripts/package-macos.sh release
bash scripts/build-host.sh release x86_64-unknown-linux-gnu
bash scripts/package-host.sh release x86_64-unknown-linux-gnu
```

Without distribution options, `package-macos.sh` builds an ad hoc development
package; notarization also needs `APPLE_API_KEY_PATH`. Debug symbols are covered in
[the hosts guide](docs/hosts.md#source-builds-and-debug-symbols).

## Host automation

Install and pair as described in [Install](README.md#install). Run `sailry help`
for all commands. Service controls are `start`, `stop`, `restart` and `status`;
`version` reports the installed release. `sailry share` shows a pairing PIN while
it runs. `sailry update` installs the newest release (`--version` selects one),
keeps `~/.sailry-host` and restores the previous program files if the update
fails.

Submit a task to the running Host:

```sh
sailry submit --worktree "$WORKTREE_ID" --prompt-file task.txt --request-id "$REQUEST_ID"
sailry submit --session "$SESSION_ID" --expected-revision "$REVISION" --stdin < task.txt
sailry wait --session "$SESSION_ID" --turn "$TURN_ID"
```

- New tasks use the Node's defaults. With `--worktree`, you can also pass
  `--config` (full session configuration JSON), `--execution model` or
  `--execution team --team "$TEAM_ID"` (with optional `--team-revision`), and
  `--association` (a `{namespace, kind, id}` JSON file).
- Retry an uncertain submission with the same request ID, input and revision
  option. `wait` observes a turn without resubmitting it. `--no-wait` returns after
  durable admission. Ctrl-C and `--timeout` end observation; execution continues.
- Each command prints one JSON result with the request, session, turn, revision,
  `state`, answer text and pending approvals or questions.
- Exit codes: `0` admitted or completed, `1` failed or stopped, `3` pending
  approval, input or paused, `4` uncertain, `124` timeout, `130` cancelled
  observation.

For CI, `scripts/ci-task.sh` wraps `submit`. Set `SAILRY_REQUEST_ID` and exactly one
of `SAILRY_WORKTREE` or `SAILRY_SESSION`, and pass the prompt on stdin or through
`SAILRY_PROMPT_FILE`. Optional variables: `SAILRY_PROFILE`, `SAILRY_TIMEOUT`,
`SAILRY_EXPECTED_REVISION`, `SAILRY_CONFIG` and `SAILRY_COMMAND`. The script never
starts a service or approves tools.

## Public source and history

Only the explicitly listed public guides in `docs/` are tracked. Internal plans,
local outputs, profiles and credentials stay ignored. Ignore rules do not remove
old Git history, so publish from a fresh repository built from one committed
snapshot:

```sh
python3 scripts/public-source.py --export /absolute/new/public-sailry
```

The destination must not exist. The exporter copies tracked source only into one
new root commit and does not set a remote or push. It checks the publication
boundary but is not a complete secret or license audit.

## Contributor License Agreement

Contributions require signing the [Sailry CLA](CLA.md), which lets the project
relicense contributions, including under commercial terms. Sign it in your first
pull request as described there. Pull requests are not merged without it.

## Licensing

Sailry-owned contributions use [Apache-2.0](LICENSE). The root license does not
relicense vendored source, imported scripts, fonts, icons or other resources.
Preserve their copyright, provenance and license notices, and include them when
redistributing.
