# Sailry

Sailry is a workspace for AI conversations, development tools and remote
execution. A shared Rust Node runs agents and owns projects, sessions, terminals,
files and credentials. Desktop can control its local Node or a paired remote Host.

Desktop uses GPUI Kit, agent execution uses ADK-Rust, and authenticated device
connections use iroh. This repository also includes a Flutter controller and a
self-hostable short-code pairing service. This is development software;
incompatible profiles are rejected, not automatically migrated or reset.

## Development

Use Rust 1.98.0, Git, Python 3.12 or newer, Zig 0.16.0 and your platform's native
build tools. The Ghostty binding builds native code with Zig. Desktop on macOS
requires Xcode Command Line Tools. Dependencies are pinned in `Cargo.toml` and
committed lockfiles; machine-local Cargo overrides are not part of the build.

From the repository root:

```sh
cargo build --locked -p sailry-host
just desktop
```

`just` is the task runner. `just desktop` prepares the office runtime and, on
macOS, builds an application bundle. Its default profile is `.runtime/desktop`.
Office preparation requires `uv` 0.10.9; it creates a pinned, relocatable Python
runtime without installing packages into your user Python environment.
An available Apple Development signing identity keeps the development app's OS
permission identity stable; otherwise the script uses ad hoc signing.

```sh
just desktop /absolute/private/desktop-profile
just host /absolute/private/host-profile
just preview
```

Direct Desktop and Host binaries default to `~/.sailry`; one running Node may own
a profile. Profiles contain Node storage, protected credentials, plugin packages,
plugin data and attachments. Projects remain at their registered paths. Remote
execution data stays on the execution Node.

Host is a long-running headless service, not an interactive agent CLI. Desktop or
Mobile controls its conversations and tools through the shared protocol. See
[Host deployment](ARCHITECTURE.md#deployment-and-operations) and the
[pairing service guide](services/pairing-relay/README.md).

`just preview` uses identified example data without starting a Node or opening a
business profile. It cannot be combined with profile or relay options.

## Guides

- [Architecture](ARCHITECTURE.md): system context, components and runtime boundaries
- [Contributing](CONTRIBUTING.md): checks, test conventions and publication
- [Engineering rules](AGENTS.md): coding and repository conventions
- [Plugin SDK](plugins/SDK.md): desktop and headless extension contracts
- [Pairing service](services/pairing-relay/README.md): verification and hosting
- [Project summary](examples/plugins/project-summary/README.md),
  [Task Notes](examples/plugins/task-notes/README.md) and
  [tool content](examples/plugins/tool-content/README.md): installable examples

## Checks

```sh
just check
just test-source
just test-plugins
just test-backend
just test-ui
```

Backend and UI tests are separate evidence. External services, OS permissions,
real devices and performance workloads require explicit opt-in acceptance tests;
deterministic tests do not establish those results.

## License

Sailry-owned code is licensed under [Apache-2.0](LICENSE). Third-party source,
resources and shell integration retain their own licenses and attribution; see
[NOTICE](NOTICE), [source notices](third_party_licenses/) and notices in `vendor/`.
Imported Ghostty shell integration includes GPL material and is not relicensed
by Sailry's Apache license.
