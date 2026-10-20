<p align="center">
  <img src="assets/branding/sailry-mark.svg" alt="Sailry Harness" width="88" />
</p>

<h1 align="center">Sailry Harness</h1>

<p align="center">English · <a href="README.zh-CN.md">Chinese (Simplified)</a></p>

<p align="center"><strong>AI agents, files and tools in one native workspace.</strong></p>

> [!NOTE]
> **Early preview.** Sailry is under active development, so expect bugs and changes between releases. Back up important work before letting agents edit it, and please [report issues](https://github.com/sailry/sailry-harness/issues) — your feedback shapes what comes next.

![Sailry in dark mode with a new conversation and example projects](assets/readme/workspace-dark.png)

Use your own models to work with files, Git, terminals, browsers, SSH and databases
beside the conversation. Add plugins to shape the workspace around your tasks.
Run locally or on your own server, and continue from Desktop or Mobile.

## Install

[Changelog](CHANGELOG.md) · [Report an issue](https://github.com/sailry/sailry-harness/issues)

### Desktop

| Platform | Architecture | Status | Download |
| --- | --- | --- | --- |
| macOS 13+ | Apple silicon / Intel | Supported | [Previews](https://github.com/sailry/sailry-harness/releases?q=prerelease%3Atrue) |
| Windows | x64 / ARM64 | Planned | — |
| Linux | x64 / ARM64 | Planned | — |

### Host

| Platform | Architecture | Status | Download |
| --- | --- | --- | --- |
| macOS 13+ | Apple silicon / Intel | Awaiting release | [Previews](https://github.com/sailry/sailry-harness/releases?q=prerelease%3Atrue) |
| Linux · glibc 2.34+ | x64 / ARM64 | Awaiting release | [Previews](https://github.com/sailry/sailry-harness/releases?q=prerelease%3Atrue) |
| Windows | — | Planned | — |

### Mobile

| Platform | Architecture | Status | Download |
| --- | --- | --- | --- |
| Android 7.0+ | ARM64 | Not released | — |
| iOS 15+ | ARM64 | Not released | — |

## Get started

Add an existing folder or clone a repository as a project on **Local device** or a connected Host.
Choose **Create session**, pick a model and describe your task; review tool approvals when requested.

## Guides

[Models](docs/models.md) · [Remote hosts](docs/hosts.md) · [Projects and sessions](docs/projects.md)

[Build from source](CONTRIBUTING.md#native-setup)

## License

Sailry-owned code uses [AGPL-3.0-only](LICENSE). The plugin SDK in [sdk/](sdk/)
and the plugin interface files listed in [NOTICE](NOTICE) use
[Apache-2.0](sdk/LICENSE), as do the official
[plugins](https://github.com/sailry/sailry-plugins) and
[themes](https://github.com/sailry/sailry-themes) repositories.

Commercial licenses are available for uses that cannot comply with the AGPL;
[contact the maintainers](https://github.com/sailry). See [COMMERCIAL.md](COMMERCIAL.md).

Third-party materials retain their own licenses; see [NOTICE](NOTICE) and
[third-party notices](third_party_licenses/).

## Contributors

[![Contributors](https://contrib.rocks/image?repo=sailry/sailry-harness)](https://github.com/sailry/sailry-harness/graphs/contributors)
