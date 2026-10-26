<p align="center">
  <img src="assets/branding/sailry-mark.svg" alt="Sailry Harness" width="88" />
</p>

<h1 align="center">Sailry Harness</h1>

<p align="center">English · <a href="README.zh-CN.md">Chinese (Simplified)</a></p>

<p align="center"><strong>You steer. Agents sail.</strong></p>

<p align="center">Bring your own models and machines. Agents work with your files, terminals, Git, SSH and databases, and you pick up from desktop or phone.</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-blue?style=for-the-badge" alt="AGPL-3.0 License"></a>
  <a href="rust-toolchain.toml"><img src="https://img.shields.io/badge/Rust-1.98-orange?style=for-the-badge&logo=rust&logoColor=white" alt="Rust 1.98"></a>
  <a href="#install"><img src="https://img.shields.io/badge/platform-macOS-lightgrey?style=for-the-badge&logo=apple&logoColor=white" alt="Platform macOS"></a>
  <a href="https://github.com/sailry/sailry-harness/stargazers"><img src="https://img.shields.io/github/stars/sailry/sailry-harness?style=for-the-badge&color=yellow" alt="GitHub Stars"></a>
  <a href="https://discord.gg/MCDktmDey"><img src="https://img.shields.io/badge/Discord-Join-5865F2?style=for-the-badge&logo=discord&logoColor=white" alt="Discord"></a>
</p>

> [!NOTE]
> **Early preview.** Sailry is under active development, so expect bugs and changes between releases. Back up important work before letting agents edit it, and please [report issues](https://github.com/sailry/sailry-harness/issues) — your feedback shapes what comes next.

![Sailry in dark mode with a new conversation and example projects](assets/readme/workspace-dark.png)

## Why Sailry?

| Feature | **Sailry** | [TRAE (China)](https://docs.trae.cn/ide_traework-to-traecode-data-migration) | [WorkBuddy](https://www.codebuddy.cn/docs/workbuddy/Overview) | [Codex](https://learn.chatgpt.com/docs/environments/modes) | [Claude Code](https://code.claude.com/docs/en/overview) | [ZCode](https://zcode.z.ai/en/docs/welcome) | [DSH](https://github.com/deepseek-ai/deepseek-harness) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Database client | ✅ | — | — | — | — | — | — |
| SSH remote | ✅ | [✅](https://docs.trae.cn/enterprise_feature-list?lang=zh) | — | [✅](https://learn.chatgpt.com/docs/remote-connections) | [✅](https://code.claude.com/docs/en/desktop) | [✅](https://zcode.z.ai/en/docs/remote-development) | ◐ |
| Plugin UI & settings | [✅](sdk/plugins.md) | [◐](https://docs.trae.cn/ide_manage-extensions) | [◐](https://www.codebuddy.cn/docs/workbuddy/Changelog) | [◐](https://learn.chatgpt.com/docs/plugins) | [◐](https://code.claude.com/docs/en/plugins/mods/overview) | [◐](https://zcode.z.ai/en/docs/plugin) | [✅](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/client/README.md) |
| Fully open source | [✅](LICENSE) | — | — | [◐](https://learn.chatgpt.com/docs/open-source) | [❌](https://github.com/anthropics/claude-code/blob/main/LICENSE.md) | — | [✅](https://github.com/deepseek-ai/deepseek-harness) |
| Remote host tasks | ✅ | [✅](https://docs.trae.cn/ide_ssh-remote) | [◐](https://www.codebuddy.cn/docs/workbuddyapp/features/Multidevice) | [✅](https://learn.chatgpt.com/docs/remote-connections) | [✅](https://code.claude.com/docs/en/desktop) | [✅](https://zcode.z.ai/en/docs/remote-development) | [◐](https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments) |
| Phone control | 🚧 | [✅](https://forum.trae.cn/t/topic/15182) | [✅](https://www.codebuddy.cn/docs/workbuddyapp/features/Multidevice) | [✅](https://learn.chatgpt.com/docs/remote-connections) | [✅](https://code.claude.com/docs/en/remote-control) | [✅](https://zcode.z.ai/en/docs/welcome) | [◐](https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments) |
| Self-hosted execution | 🚧 | — | — | [◐](https://learn.chatgpt.com/docs/app-server) | [◐](https://code.claude.com/docs/en/desktop) | [◐](https://zcode.z.ai/en/docs/remote-development) | [✅](https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments) |
| Bring your own model | [✅](docs/models.md) | [✅](https://docs.trae.cn/ide_models) | [✅](https://www.codebuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/Model) | [✅](https://learn.chatgpt.com/docs/config-file/config-advanced) | ✅ | [✅](https://zcode.z.ai/en/docs/configuration) | [✅](https://deepseek-harness.github.io/deepseek-harness/en/guide/providers) |
| Git worktrees | [✅](docs/projects.md) | [✅](https://docs.trae.cn/enterprise_feature-list?lang=zh) | ✅ | ✅ | ✅ | — | [◐](https://github.com/deepseek-ai/deepseek-harness/releases) |

✅ Supported · ◐ Partial · 🚧 Coming soon · ❌ Unavailable · — Unconfirmed. Includes official extensions; DSH = DeepSeek Harness.

- **Keep tools together.** Projects, terminals, SSH and databases sit beside the conversation.
- **Extend the workspace.** Plugins add pages, panels, settings, tools and skills via the [Apache-2.0 SDK](sdk/plugins.md).
- **Choose models and hosts.** Use your own model services and connect [execution hosts](docs/hosts.md).

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

## Community

For collaboration or to join the group chat, add me on WeChat or join the Discord community.

<p align="center">
  <img src="assets/readme/wechat.png" width="240" alt="WeChat QR code">
  <br>
  <a href="https://discord.gg/MCDktmDey"><img src="https://img.shields.io/badge/Discord-Join%20the%20community-5865F2?style=for-the-badge&logo=discord&logoColor=white" alt="Join the Sailry Discord community"></a>
</p>

> Please use [GitHub Issues](https://github.com/sailry/sailry-harness/issues) for bug reports and feature requests so they are easier to track.

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
