<p align="center">
  <img src="assets/branding/sailry-mark.svg" alt="Sailry" width="88" />
</p>

<h1 align="center">Sailry</h1>

<p align="center">
  English · <a href="README.zh-CN.md">Chinese (Simplified)</a>
</p>

<p align="center"><strong>Your AI workspace for everyday work</strong></p>

<p align="center">
  Conversations, files, and tools. One place to move things forward.
</p>

<p align="center">
  <a href="https://github.com/sailry/sailry-harness/releases">Releases</a> ·
  <a href="CHANGELOG.md">What's new</a> ·
  <a href="https://github.com/sailry/sailry-harness/issues">Feedback</a>
</p>

![Sailry workspace with an English conversation and weekly plan](assets/readme/workspace.png)

Sailry brings AI conversations into the workspace where your work happens.
Start with an idea, turn notes into a plan, explore your files, or work through a
project with an agent—without losing the thread between chat and tools.

## Start with what you need

| Make sense of things | Create something useful | Work on a project |
| --- | --- | --- |
| Summarize notes, explore questions, and organize next steps | Shape a draft, prepare a brief, or work with documents | Browse files, review changes, and run tools alongside your conversation |

## Keep your work in view

Your projects and conversations stay on the left. Your current conversation sits
in the center. Open files and previews alongside it, so you can keep the context
close while you work.

![An English launch brief open alongside a Sailry conversation](assets/readme/files.png)

## Make it your workspace

- **Choose your model.** Connect your preferred provider or an OpenAI-compatible endpoint
- **Bring your files.** Work with your own folders, with file browsing and previews built in
- **Add useful tools.** Extend your workspace with plugins, skills, and MCP connections
- **Stay in control.** Choose when agents can edit files or run tools
- **Work remotely.** Connect a Sailry Host on another computer from the same desktop workspace
- **Keep things together.** Conversations, terminals, files, and project tools share one place

![Sailry's official plugin marketplace in English](assets/readme/plugins.png)

## Get started

Sailry is in active development. Check [Releases](https://github.com/sailry/sailry-harness/releases)
for available macOS builds, distributed outside the Mac App Store.

1. Choose the macOS package for your Mac: Apple silicon or Intel
2. Unzip it and drag **Sailry.app** into **Applications**
3. Open Sailry and connect a model provider
4. Add a folder or start a conversation

Screenshots show a fresh English-language demo workspace with illustrative
conversations, not personal data or previous test sessions.

## Help shape Sailry

Found a problem or have an idea? [Open an issue](https://github.com/sailry/sailry-harness/issues).
For changes to the application, start with the [contributing guide](CONTRIBUTING.md).

<details>
<summary>For contributors and plugin authors</summary>

- [Native setup and checks](CONTRIBUTING.md#native-setup)
- [Architecture](ARCHITECTURE.md)
- [Engineering rules](AGENTS.md)
- [Plugin SDK](sdk/plugins.md)
- [Official plugins](https://github.com/sailry/sailry-plugins)
- [Host and pairing](services/pairing-relay/README.md)

</details>

## License

Sailry-owned code is licensed under [Apache-2.0](LICENSE). Third-party source,
resources and shell integration retain their own licenses and attribution; see
[NOTICE](NOTICE), [source notices](third_party_licenses/) and notices in `vendor/`.
Imported Ghostty shell integration includes GPL material and is not relicensed
by Sailry's Apache license.
