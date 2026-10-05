# Contributing

Read [ARCHITECTURE.md](ARCHITECTURE.md) and [AGENTS.md](AGENTS.md) before changing
ownership, execution or UI boundaries. Work from the committed implementation,
preserve unrelated local changes and keep each task cohesive and reviewable.

## Setup and checks

Follow [README.md](README.md#development) for native prerequisites. Use committed
lockfiles and `--locked` for Rust commands. Do not replace pinned dependencies with
absolute local paths. Plugin JavaScript tests require Node.js 22 or newer; source
checks use Python's standard library. Pairing-service dependencies and checks have
their own [guide](services/pairing-relay/README.md#run-and-verify).

Clone with `git clone --recurse-submodules` or run
`git submodule update --init --recursive` before building. Official package changes
are committed and pushed in `sailry-plugins`; then commit the reviewed `plugins/`
gitlink update in this repository. Never copy plugin source into a second owner.

```sh
just check          # Rust formatting and Clippy with warnings denied
just test-source    # Public guides, publication boundaries and source fixtures
just test-plugins   # Deterministic package JavaScript tests
just test-backend   # Protocol, Link, Client, Node and Host tests
just test-ui        # Desktop behavior and Kit interaction tests
```

Select checks from affected behavior and dependency boundaries. Reuse passing
results while their inputs remain unchanged. Explain which tests ran and any
coverage gap; do not describe a fixture or compile check as live acceptance.

## Test conventions

The conventions follow the [pinned ADK contributor guide](https://github.com/dux-web/adk-rust/blob/b3e360bbf49ca39d49cccba182c0f7cb4d251970/CONTRIBUTING.md),
adapted to Sailry's ownership and local/remote boundaries.

- Put small Rust unit tests in nearby `#[cfg(test)]` modules. Put cross-module
  integration tests and substantial fixtures in focused test modules or the
  owning crate's `tests/` directory.
- Group tests by behavior. Use concise names distinguishing the scenario,
  rather than repeating the full crate and feature name.
- Cover successful and meaningful failure paths for changed public behavior.
  A regression test must fail without its fix. Compare complete structured
  outcomes when stable; proving only that code did not panic is insufficient.
- Keep ordinary tests deterministic and self-contained. Use isolated temporary
  profiles, injected clocks or bounded waits, and clean up spawned workers.
  Do not depend on ignored output directories, user projects or private profiles.
- Test conflicts, reconnect and original-request recovery where a changed command
  can cause side effects. Exercise shared behavior through local and remote paths,
  not merely a mocked transport.
- Make external prerequisites explicit. Rust live-service/device tests use
  `#[ignore = "requires …"]` with a focused opt-in command. Do not silently return
  success when credentials, services or devices are missing. An explicitly gated
  subprocess fixture is not an independently passing acceptance test.
- Keep test credentials synthetic, use loopback fixtures by default and never
  log secrets. Preserve intentional Unicode and malformed-input fixtures.
- Use property tests for meaningful invariants when existing test tools fit.
  Do not add a framework or arbitrary coverage targets to inflate test counts.
- Keep source and developer diagnostics in English. Product copy belongs in
  i18n resources; test operation feedback through the toast boundary.

Real-service, device, OS interaction and performance acceptance must state their
actual prerequisites and results separately. Screenshots are not a default gate.

## Continuous integration

[GitHub Actions](.github/workflows/ci.yml) runs public-source fixtures, all packaged
JavaScript tests and pairing-service checks on Linux. macOS jobs run strict Rust
checks, backend local/remote integration, desktop Kit interactions, Flutter widget
tests, native Dart FFI contracts and Flutter against an isolated real remote Node.
Actions and toolchains are pinned; Rust and Flutter use committed lockfiles.

CI does not supply model credentials, production profiles or OS permissions.
Live-service and device tests stay opt-in. The ordinary Flutter suite skips its
bootstrap-dependent native fixture, which the explicit remote-Node step runs.
The FFI step excludes the MCP subprocess entry point and database contracts that
require PostgreSQL/MySQL executables. Run `just test-mobile-contract` with those
executables available to include the database contracts. A macOS host-side Flutter
run does not establish iOS or Android device acceptance.

## Public source and history

Internal `docs/`, plans, local outputs, profiles and credentials are ignored and
must not be tracked. Public guides, `AGENTS.md`, package Skills, fixtures, licenses
and lockfiles remain source. Ignore rules alone do not remove old Git history.

Keep private development history private. After committing the reviewed public
tree, create a new local repository from that one committed snapshot:

```sh
python3 scripts/public-source.py --export /absolute/new/public-sailry
```

The destination must not exist. The exporter checks the committed snapshot,
copies tracked source only and creates one new root commit. It does not copy
private history, uncommitted changes or ignored files, and does not set a remote
or push. Review the new repository before selecting a public remote. This is a
publication boundary check, not a complete secret or license audit.

## Licensing

Sailry-owned contributions use [Apache-2.0](LICENSE). Preserve third-party
copyright, source provenance and license notices. The root license does not
relicense vendored source, imported shell scripts, fonts, icons or other resources.
Include applicable notices when packaging or redistributing those materials.
