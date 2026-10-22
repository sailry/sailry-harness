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

## Why Sailry?

- **Choose models per conversation.** Connect your own provider accounts or
  compatible API endpoints, and keep the same project and tools when you
  [switch models](docs/models.md).
- **Extend the workspace.** Plugins can add pages, panels, settings, agent tools
  and skills through a public [Apache-2.0 SDK](sdk/plugins.md).
- **Control where tasks run.** Desktop includes local execution. The standalone
  [Host](docs/hosts.md) uses the same runtime on your own machine and keeps accepted
  tasks running when the controller disconnects.

The products below offer overlapping capabilities with different workflows and
extension options. Links lead to their official documentation.

| Product | Workflow | Customization |
| --- | --- | --- |
| **Sailry** | Native desktop workspace for conversations, projects and tools | Custom model endpoints; plugins for UI, tools and skills; MCP |
| [TRAE (China edition)](https://docs.trae.cn/ide_traework-to-traecode-data-migration) | Integrated Agent and IDE for software development | [Built-in and custom models](https://docs.trae.cn/ide_models); skills, MCP and plugins |
| [WorkBuddy](https://www.codebuddy.cn/docs/workbuddy/Overview) | Desktop tasks across office work, coding and content | [Custom models, including Ollama](https://www.codebuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/Model); skills and [MCP/CLI connectors](https://open.workbuddy.cn/docs/connector) |
| [Codex](https://learn.chatgpt.com/docs/environments/modes) | Coding across local projects, worktrees and cloud environments | [Custom providers, model gateways and local models](https://learn.chatgpt.com/docs/config-file/config-advanced); [skills, MCP and plugins](https://learn.chatgpt.com/docs/skills-and-plugins) |
| [Claude Code](https://code.claude.com/docs/en/overview) | Coding from the terminal, IDE, desktop or browser | Skills, MCP and plugins; supported provider and gateway deployments |
| [ZCode](https://zcode.z.ai/en/docs/welcome) | Development workspace optimized for GLM | [Third-party providers](https://zcode.z.ai/en/docs/configuration); skills, MCP, hooks and [plugins](https://zcode.z.ai/en/docs/plugin) |
| [DeepSeek Harness (DSH)](https://www.deepseek.com/en/harness/) | Desktop and web workspace for coding and everyday tasks | [Custom providers](https://deepseek-harness.github.io/deepseek-harness/en/guide/providers); plugins for models, tools, skills and UI |

Sailry is an early preview: macOS Desktop is available, Host packages are awaiting
release, and Mobile is not yet released. See [platform availability](#install).

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
