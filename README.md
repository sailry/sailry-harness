# Sailry Harness

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square" alt="Apache-2.0 License"></a>
  <a href="https://github.com/sailry/sailry-harness/stargazers"><img src="https://img.shields.io/github/stars/sailry/sailry-harness?style=flat-square&color=yellow" alt="GitHub Stars"></a>
  <a href="https://discord.gg/MCDktmDey"><img src="https://img.shields.io/badge/chat-Discord-5865F2?style=flat-square&logo=discord&logoColor=white" alt="Discord"></a>
  <a href="https://awesomemygo.dev/apps/sailry"><img src="https://awesomemygo.dev/badge.svg" alt="Listed on Awesome MyGo"></a>
</p>

<p align="center">
  <a href="README.zh-CN.md">简体中文</a> ·
  <a href="ARCHITECTURE.md">Architecture</a> ·
  <a href="CONTRIBUTING.md">Contributing</a> ·
  <a href="sdk/plugins.md">Plugin SDK</a> ·
  <a href="AGENTS.md">Engineering Rules</a> ·
  <a href="https://github.com/sailry/sailry-code">Sailry Code (Product)</a>
</p>

**Sailry Harness** (`github.com/sailry/sailry-harness`) is the open-source AI agent runtime framework, public Go/Flutter/Plugin SDK, and runnable general-purpose base application. It provides the local and remote execution Node, end-to-end encrypted peer-to-peer Link v2 transport, Client state projection and recovery, native GPU Desktop shell and Kit ([MyGo](https://github.com/egoist/mygo)), Flutter Mobile controller core, and 21 general modules under [`modules/`](modules/).

> [!TIP]
> **Looking to install and use Sailry as an AI coding workstation?**
> Download end-user Desktop, Host, and Mobile packages from **[Sailry Code](https://github.com/sailry/sailry-code)** ([Releases](https://github.com/sailry/sailry-code/releases)), the official downstream workstation built on Sailry Harness that adds native Files workbench, Git & worktrees, Terminals, Tree-sitter Code Graph, Databases, and SSH.

> [!NOTE]
> **Early preview.** Sailry Harness APIs and runtime internals evolve quickly between tags. Report framework bugs and SDK issues in [GitHub Issues](https://github.com/sailry/sailry-harness/issues).

## Architecture

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="Desktop and mobile Clients control one or more Nodes over end-to-end encrypted libp2p Link v2 connections with Circuit Relay v2 fallback. Each Node owns execution, tools, approvals, credentials, and canonical history.">
</p>

- **Node (`internal/node/`)**: Owns the agent loop, tool execution, session permissions/approvals, model routing, credentials stored in the private profile, and canonical SQLite/Turso history. Runs embedded inside Desktop or headless via Host.
- **Link v2 & Relays (`internal/link/`)**: End-to-end encrypted libp2p peer-to-peer transport with six-digit pairing codes and automatic Circuit Relay v2 fallback — no port forwarding, VPN, or cloud account required.
- **Client (`internal/client/`)**: Projects streams and snapshots for local UI and remote devices, handles durable reconnect/recovery, and exposes uniform host/resource queries.
- **Desktop Shell & Kit (`internal/desktop/`)**: Native GPU interface powered by [MyGo](https://github.com/egoist/mygo) (no Electron or bundled browser engine). Modules mount pages, workspace panels, conversation panels, and settings cleanly into shared shell slots.
- **Mobile Core (`apps/mobile/`)**: Flutter controller packages (`sailry_mobile` + `sailry_bridge` via Dart FFI) extended by downstream products through `MobileAssembly`.
- **General Modules (`modules/`)**: 21 reusable Desktop/Node feature modules compiled directly without JS runtime overhead unless explicitly packaged as plugins.

See [ARCHITECTURE.md](ARCHITECTURE.md) for full component dependency rules, ownership boundaries, storage/security invariants, and GUI resource lifecycle rules.

## Products and ecosystem

| Repository | Role | License |
| --- | --- | --- |
| **[Sailry Code](https://github.com/sailry/sailry-code)** | Official open-source AI developer workstation assembled on `sailry-harness`; owns 7 native developer modules (`code-index`, `files`, `git`, `worktrees`, `terminal`, `databases`, `ssh`), product identity (`~/.sailry-code`), and end-user releases | Apache-2.0 |
| **[sailry-plugins](https://github.com/sailry/sailry-plugins)** | Official optional runtime plugins (zero plugins are preinstalled by default) | Apache-2.0 |
| **[sailry-themes](https://github.com/sailry/sailry-themes)** | Official light/dark semantic UI themes | Apache-2.0 |

## SDKs and extension points

Downstream Go/Flutter products depend **one-way** on [`sdk/`](sdk/) and `apps/mobile/` packages (never `internal/*`, enforced in CI), and Harness keeps default profiles free of preinstalled plugins or themes:

| Layer | Public Entry Points | What You Build |
| --- | --- | --- |
| **Go Desktop & Product Assembly** | `sdk/desktop`, `sdk/desktop/harness`, `sdk/desktop/native`, `sdk/desktop/controls`, `sdk/desktop/theme`, `sdk/desktop/view`, `sdk/desktop/settings`, `sdk/desktop/i18n`, `sdk/desktop/icons`, `sdk/desktop/testing`, `sdk/product`, `sdk/release` | Custom branded Desktop binaries, custom theme/identity/updater keys, and native MyGo views/settings |
| **Go Node & Protocol** | `sdk/host`, `sdk/native/node`, `sdk/native/protocol`, `sdk/native/files`, `sdk/native/codeindex`, `sdk/native/link`, `sdk/native/memory`, `sdk/native/process`, `sdk/native/search`, `sdk/native/storage`, `sdk/sdktest` | Custom Host binaries, native Node agent tools, scoped workspace/resource services, and contract tests |
| **Flutter Mobile Core** | `apps/mobile/` (`sailry_mobile`, `sailry_bridge`, `MobileAssembly`) | Domain-branded Android/iOS controllers adding custom project resource sheets |
| **Runtime Plugins (`moejs` / MCP)** | [sdk/plugins.md](sdk/plugins.md), `sdk/desktop/*.d.ts`, `sdk/node/*.d.ts` | Cross-platform JS UI pages/panels + Node callbacks using [agent-plugins](https://agent-plugins.org), `SKILL.md`, and MCP |

## Built-in general capabilities

Out of the box (including when launched standalone as the base app), Sailry Harness includes:

- **Work modes & parallel subagents**: Standard, Plan (read-only planning), and Collaborate (parallel child agents with optional role/model overrides), all visible with approval gating.
- **Core workspace tools**: Sandboxed command execution (`run_command`, Linux Bubblewrap + OS confinement), path-confined file read/write/edit/patch, and file search.
- **Browser & computer use**: Built-in headless/interactive browser (`browser`), external browser automation (`external-browser`), and desktop computer control (`computer`).
- **Documents, search & media**: Word/Excel/PowerPoint/PDF reading with local OCR, interactive HTML artifact previews, image/video media generation (`media`), web search (`web-search`), and local knowledge collections.
- **Continuity & local speech**: Long-term memory, searchable sessions, goals, reminders, scheduled tasks (`dispatch`), session usage statistics, provider account quota status, and local voice dictation.

<a id="install"></a>
## Quick start

Follow [CONTRIBUTING.md](CONTRIBUTING.md#setup) to install prerequisites (**Go 1.27.3**, **Rust** for the local Turso library, **Flutter** for mobile work, and Linux **Bubblewrap** for command sandboxes), then build native assets and launch from source:

```sh
just native
just desktop        # opens base app against ~/.sailry-harness
just desktop dev    # opens isolated dev profile ~/.sailry-harness-dev
just host           # runs headless Host against .runtime/profiles/host
just check          # checks formatting, builds, vets, and runs tests
```

- **Co-developing with Sailry Code**: Create a gitignored root `go.work` inside your local [Sailry Code](https://github.com/sailry/sailry-code) checkout referencing `../sailry-go`; Code's `dev.sh` automatically tests your local Harness changes end-to-end.
- **Releases**: Historical Harness binaries remain in [Sailry Harness Releases](https://github.com/sailry/sailry-harness/releases) for explicit repair builds only; new end-user installers ship exclusively from [Sailry Code Releases](https://github.com/sailry/sailry-code/releases).

## Documentation

- **Framework & engineering**: [Architecture](ARCHITECTURE.md) · [Contributing & CI](CONTRIBUTING.md) · [Engineering Rules](AGENTS.md) · [Plugin SDK v1](sdk/plugins.md) · [Security Policy](SECURITY.md)
- **Shared runtime guides** (synced to `sailry.dev/docs/`): [Models](docs/models.md) · [Remote Hosts](docs/hosts.md) · [Projects & Sessions](docs/projects.md) · [Relays](docs/relays.md)

## Telemetry, community, and license

By default, running binaries send anonymous error reports and anonymous feature counts (never conversations, files, credentials, paths, or custom names); disable either in **Settings → General → Diagnostics and privacy** or via `SAILRY_TELEMETRY=off` / `DO_NOT_TRACK=1`. Public model benchmarks come from [ModelDial](third_party_licenses/modeldial.md).

Join the [Discord](https://discord.gg/MCDktmDey) or scan the WeChat QR code below, and report bugs in [GitHub Issues](https://github.com/sailry/sailry-harness/issues).

<p align="center">
  <img src="assets/readme/wechat.png" width="200" alt="WeChat QR code for the Sailry community">
</p>

Licensed under [Apache-2.0](LICENSE) (including [`sdk/`](sdk/), [sailry-plugins](https://github.com/sailry/sailry-plugins), and [sailry-themes](https://github.com/sailry/sailry-themes)). Sailry names and logos are covered separately by [TRADEMARKS.md](TRADEMARKS.md); third-party notices live in [NOTICE](NOTICE) and [third_party_licenses/](third_party_licenses/).

[![Contributors](https://contrib.rocks/image?repo=sailry/sailry-harness)](https://github.com/sailry/sailry-harness/graphs/contributors)
