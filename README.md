<p align="center">
  <img src="assets/branding/sailry-mark.svg" alt="Sailry Harness" width="88" />
</p>

<h1 align="center">Sailry Harness</h1>

<p align="center">
  English · <a href="README.zh-CN.md">Chinese (Simplified)</a>
</p>

<p align="center"><strong>A native AI workspace, designed to be light, beautiful, and extensible</strong></p>

<p align="center">
  Your models and tools, in a workspace you can make your own.
</p>

<p align="center">
  <a href="https://github.com/sailry/sailry-harness/releases">Releases</a> ·
  <a href="CHANGELOG.md">What's new</a> ·
  <a href="https://github.com/sailry/sailry-harness/issues">Feedback</a>
</p>

![Sailry workspace with an English conversation and weekly plan](assets/readme/workspace.png)

Sailry Harness brings **AI agents and the tools they work with into a native
workspace**. Choose a model, connect a project, and work alongside the agent:
inspect files, review diffs, use a terminal, browse sources, or query a database
beside the conversation.

Bring your preferred models and shape the workspace with plugins and themes.
Work on your computer or your own server, and return from Desktop or Mobile.

## Native. Light by design. Beautiful. Extensible.

### Native rendering, throughout the workspace

The desktop shell and plugin panels use **Rust and GPU-rendered GPUI Kit
controls**, with shared themes, focus, and keyboard behavior. Ghostty powers the
embedded terminal. Web content and document previews have their own embedded
webviews.

Files, diffs, terminals, and conversations live together in the app. Plugin
surfaces use the same native components as the rest of the workspace.

### A focused core, with tools you choose

Rust owns execution, connections, and shared state. Feature workbenches come from
plugins: install the tools your work needs and extend from there. Selected
plugins ship with the app for offline installation and can update independently.

Desktop includes a local execution Node. Host runs that same engine as a service;
Mobile carries the shared client and controller UI. Execution stays on the
selected machine as you change controllers.

### Designed to look good while you work

Projects and conversations on the left. The agent in the center. Files, previews,
and tools on the right. Keep the conversation and its results visible together:
read a change while discussing it, or inspect query results beside the database
assistant.

Choose light or dark mode, follow your system appearance, or import a
[theme package](themes/README.md). Themes can customize colors, backgrounds,
and visual accents; native controls and plugin panels share the application
theme.

![An English launch brief open alongside a Sailry conversation](assets/readme/files.png)

### Extend the whole experience

A plugin can contribute **tools, navigation, resource panels, embedded
assistants, and composer actions**. Files, Git, terminals, SSH, and databases use
this same extension system. Build a database workbench, a server assistant, or a
specialized workflow with both agent tools and a native interface.

Skills supply reusable workflows. MCP connects tool servers. Sailry plugins also
shape the workspace where you use them. Start with the
[official plugins](https://github.com/sailry/sailry-plugins), or build with the
[Plugin SDK](sdk/plugins.md).

Desktop, Host, Mobile, and official plugin source are open under Apache-2.0,
with third-party licenses preserved.

## Put it to work

| Your task | What you work with in Sailry |
| --- | --- |
| Refactor a repository | Project files, Git diffs, worktrees, terminals, and code review beside the agent |
| Research and prepare a brief | Browser, web search, source files, and document previews in the same workspace |
| Investigate a server or database | SSH and database resource panels with assistants scoped to those connections |
| Keep a long task on a server | Start it on Host from Desktop; reconnect from Desktop or Mobile to review progress and guide it |
| Build a specialized assistant | A plugin with its own tools, native panel, embedded conversation, and Node-owned data |

## Keep control as the agent works

- **Plan and review.** Explore in Plan mode, review the plan, then explicitly
  switch to making changes
- **Guide a running task.** Send guidance during execution; it takes effect after
  the current tool batch, keeping the conversation intact
- **Delegate with context.** Subagents have their own roles and models, retain
  history for follow-up work, and can run in the background. The main agent
  guides and waits for them; their approvals appear in its conversation
- **Choose permissions.** Configure tool access per session. Plans, permissions,
  and execution remain separate choices

## One task, multiple controllers

Run locally with the Node built into Desktop, or connect to **Sailry Host on your
own server**. Desktop's **Add Host** installs the service over SSH and pairs it
automatically. Keep a long task on Host, close the desktop controller, and return
to the same conversation from Desktop or Mobile.

The mobile app connects directly to a paired Node over an **end-to-end encrypted
link**, without a Sailry account or desktop gateway. The Node must remain running
and awake: the local Node lives inside Desktop, while Host is a separate service.

```text
          ┌─ in-process ─── local Node inside Desktop
Desktop ──┤
          └─ encrypted ───┐
                          ├── Sailry Host on your server
Mobile ──── encrypted ────┤
                          └── local Node of a paired Desktop
```

Pair with a six-digit code that expires in 60 seconds. Paired devices reconnect
without another code. The pairing service exchanges invitation tickets;
conversations travel over the encrypted Node link. An encrypted data relay can
carry traffic when a direct connection is unavailable.

Reconnect to the Node's saved conversation and task state. Disconnecting a
controller does not cancel admitted work; stopping a task is an explicit action.
Sailry reports uncertain outcomes without automatically repeating side effects.
See [Architecture](ARCHITECTURE.md) for execution, recovery, and data boundaries.

## Build your workspace with plugins

| Work | Official plugins |
| --- | --- |
| Code | Files, Git, Worktrees, Commands and terminals, Code review, GitHub |
| Research and documents | Browser, Web search, Office documents, Media, Context7 |
| Servers and data | SSH, Databases |
| Agent workflow | Delegation, Goals, Progress, Memory, Scheduled tasks, Reminders, Usage statistics |
| Your computer | Computer control, External browsers |
| Downtime | Xiangqi, Gomoku, Reversi, Dou Dizhu, Texas Hold'em, Liar's Dice, City Trader |

![Sailry's official plugin marketplace in English](assets/readme/plugins.png)

Tool availability follows the selected Node, controller, platform, and permissions.

## Connect your models

| Connection | Built-in choices |
| --- | --- |
| Core model services | OpenAI, Anthropic, Google Gemini |
| Additional hosted providers | xAI, DeepSeek, Qwen, Moonshot (Kimi), Mistral, MiniMax, Doubao, Zhipu, Baidu, Cohere |
| Multi-model services | OpenCode Go and OpenCode Zen |
| Cloud integrations | Azure OpenAI, Amazon Bedrock, Google Vertex AI |
| Custom-compatible endpoints | OpenAI Responses, OpenAI Chat Completions, Anthropic Messages, Gemini generateContent |

Use your own keys, a compatible gateway or local endpoint, or ChatGPT sign-in
where offered. Give subagent roles different models and instructions for
exploration, implementation, and review.

Provider credentials stay in the selected Node's protected storage. Requests go
to the provider you choose; a local endpoint lets you use local inference. Model
availability, reasoning, media, and tool support depend on the provider and model.

## Platforms

| Platform | Minimum requirements | Application | Status |
| --- | --- | --- | --- |
| macOS | macOS 13.0 Ventura or later; Apple silicon or Intel | Desktop workspace | Supported |
| Linux | amd64 or arm64; systemd and glibc 2.28 or later | Sailry Host service | Supported |
| Android | Android 7.0 or later (API 24); ARM64 | Mobile controller | Supported |
| Windows | System requirements under validation | Desktop workspace | In testing |
| iOS | iOS 15.0 or later | Mobile controller | In testing |

Mobile controllers connect to a computer running Sailry Desktop or Sailry Host.
Platform support does not mean every platform already has a published installer;
see [Releases](https://github.com/sailry/sailry-harness/releases).

## Get started

Sailry Harness is in active development. macOS builds are distributed through
[Releases](https://github.com/sailry/sailry-harness/releases), outside the Mac App
Store.

1. Download the DMG for your Mac: Apple silicon or Intel
2. Open it and drag **Sailry.app** into **Applications**
3. Open Sailry and connect a model provider
4. Add a folder or start a conversation

ZIP assets on the same release serve Desktop's automatic updater.

Screenshots show a fresh English-language demo workspace with illustrative
conversations, not personal data or previous test sessions.

### Install Sailry Host

Desktop's **Add Host** installs and pairs over SSH. For manual installation:

<details>
<summary>Linux requirements, installation, and service commands</summary>

Linux Host is a separate release asset for **amd64** and **arm64**. It requires
systemd, glibc 2.28 or newer, and the X11/XInput client libraries
(`libX11.so.6` and `libXi.so.6`); no desktop session is needed for headless
tasks. On a minimal Debian or Ubuntu server:

```sh
sudo apt-get install libx11-6 libxi6
curl -fsSL https://github.com/sailry/sailry-harness/releases/download/v0.1.0-alpha.1/install-host.sh | bash -s -- --version 0.1.0-alpha.1
```

The installer verifies the archive's release checksum and starts a persistent
service. Run it as the account that should own the Host; root uses a system
service, other accounts use a user service with lingering enabled, which may
require administrator access. Existing Host files and profiles are not
overwritten. Draft release assets are unavailable to the installer until the
release is published.

```sh
sailry version
sailry start
sailry stop
sailry restart
sailry status
sailry share
sailry update
sailry update --version 0.1.0-alpha.1
```

`share` asks the running Host for a private pairing PIN; keep it running until
pairing completes, or press Ctrl-C to cancel. `update` downloads a published
version, preserves `~/.sailry-host`, and restarts the same service, keeping the
previous program files in a reported backup directory. Without `--version`,
installation and updates select the newest published release, including
previews. A failed startup leaves the profile unchanged. For non-root
installations, add `~/.local/bin` to PATH if needed:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Desktop's **Add Host** flow runs the same official installer over SSH, pinned to
the desktop's release version, verifies the installer and archive checksums, and
then pairs with the service. For an existing Host, run `sailry share` and pair
instead of installing again.

</details>

## Help shape Sailry Harness

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

## Acknowledgements

Thanks to the open-source projects and their contributors that make Sailry
Harness possible, especially:

- [GPUI Kit](https://github.com/longbridge/gpui-kit) by Longbridge — desktop components, themes, and native plugin UI
- [GPUI](https://gpui.rs/) by the Zed team — GPU-accelerated desktop rendering
- [ADK-Rust](https://github.com/zavora-ai/adk-rust) — agent execution and model integrations
- [iroh](https://github.com/n0-computer/iroh) — encrypted connectivity between devices
- [Ghostty](https://github.com/ghostty-org/ghostty) — the embedded terminal renderer and shell integration
- [Wry](https://github.com/tauri-apps/wry) and GPUI Kit's WebView integration — embedded browser panes
- [rquickjs](https://github.com/DelSkayn/rquickjs) and [QuickJS](https://bellard.org/quickjs/) — headless plugin callbacks
- [Flutter](https://github.com/flutter/flutter) — the mobile controller

We also thank the maintainers of the other libraries, fonts, icons, and document
tools used here. Attribution and license notices remain in
[NOTICE](NOTICE), [third-party notices](third_party_licenses/), and `vendor/`.

## License

Sailry-owned code is licensed under [Apache-2.0](LICENSE). Third-party source,
resources and shell integration retain their own licenses and attribution; see
[NOTICE](NOTICE), [source notices](third_party_licenses/) and notices in `vendor/`.
Imported Ghostty shell integration includes GPL material and is not relicensed
by Sailry's Apache license.
