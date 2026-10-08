<p align="center">
  <img src="assets/branding/sailry-mark.svg" alt="Sailry Harness" width="88" />
</p>

<h1 align="center">Sailry Harness</h1>

<p align="center">
  English · <a href="README.zh-CN.md">Chinese (Simplified)</a>
</p>

<p align="center"><strong>A native AI workspace for coding and everyday work</strong></p>

<p align="center">
  Agents run on your own machines. Pick up the work from any device.
</p>

<p align="center">
  <a href="https://github.com/sailry/sailry-harness/releases">Releases</a> ·
  <a href="CHANGELOG.md">What's new</a> ·
  <a href="https://github.com/sailry/sailry-harness/issues">Feedback</a>
</p>

![Sailry workspace with an English conversation and weekly plan](assets/readme/workspace.png)

Sailry Harness is an **AI agent harness**: it brings models, agents, context,
tools, and permissions together in one native workspace, and lets you follow and
steer the work as it happens.

Use it to write and review code, research a topic, prepare documents, query a
database, or keep a long task running on a server. You choose the model provider
and the computer that does the work. Sailry supplies the workspace around it.

## Why Sailry

Most AI agent apps tie the work to the window in front of you: close it, switch
devices, or lose the connection, and the session stays behind. The alternative
is usually a vendor's cloud. Sailry separates the workspace you see from the
**execution Node** that does the work, and lets you own both.

Start a refactor on your Mac, let it run on your own Linux server overnight,
and check on it from your phone the next morning—the same conversation, files,
and settings, on hardware you control.

- **Run work on machines you own.** Use the Node built into Desktop, or install
  Sailry Host as a headless service on your own server. Desktop's **Add Host**
  sets it up over SSH and pairs automatically
- **Continue from any device.** Desktop and the mobile app reconnect to the same
  Node-owned workspace, conversations, and session settings—not a copied chat
  history. Phones connect to the Node directly over an end-to-end encrypted
  link, with no desktop gateway and no Sailry account
- **Disconnects don't lose work.** Commands are durably admitted before they
  are acknowledged. Closing a controller only stops observation; admitted work
  keeps running, and uncertain outcomes are reported instead of replayed
- **Native from shell to plugins.** A Rust desktop app with GPU-rendered GPUI
  Kit controls, not Electron. Plugin panels use the same native components;
  webviews appear only inside browser panes
- **Bring any model.** 14 built-in provider brands and services, custom endpoints
  for four common API protocols, local models through compatible gateways, and
  ChatGPT sign-in where offered. Delegated subagents can each use their own model
- **Plugins build whole workbenches.** A plugin can add agent tools, navigation
  entries, panels, embedded assistants, and composer actions. Files, Git,
  terminals, the browser, SSH, and databases are plugins too, on the same SDK
  that is open to you
- **Open source end to end.** Desktop, Host, the mobile controller, and the
  official plugins are Apache-2.0 licensed

## Run work where it belongs

Desktop includes a local execution Node, so you can start without any setup.
When the files, environment, or long-running work belong somewhere else, connect
a computer running Sailry Host. Host is a long-running service, not a terminal
session: it keeps working after you close your laptop.

```text
          ┌─ in-process ─── local Node inside Desktop
Desktop ──┤
          └─ encrypted ───┐
                          ├── Sailry Host on your server
Mobile ──── encrypted ────┤
                          └── local Node of a paired Desktop
```

- **Pair once.** A six-digit code that expires in 60 seconds establishes trust;
  paired devices reconnect without it. The pairing service only exchanges that
  ticket; it never sees your conversations
- **Same commands everywhere.** Local and remote work use one command model, so
  conversations, files, Git, and terminals behave the same on either Node
- **Credentials stay put.** Provider keys live in the execution Node's protected
  storage, never in events or controller caches
- **Recover from durable history.** Reconnecting resumes from the Node's
  history, using the original request IDs

The Flutter mobile controller is available for Android; iOS is in testing.
Mobile steers work on the connected Node rather than running a second agent
engine on the phone.

## Agents you can steer

- **Plan, then build.** Plan mode explores without side effects, and every plan
  is reviewed before the agent switches to making changes
- **Guide a running task.** Send guidance mid-run; it is applied after the
  current tool batch, without restarting the conversation
- **Delegate.** Hand subtasks to subagents, each with its own role, model, and
  worktree—for example, a fast model to explore and a stronger one to review.
  Subagents can run in the background while the main agent guides and waits for
  them, and they keep their history for follow-up work
- **Stay in control.** Choose tool permissions per session; approvals from
  subagents appear in the main conversation

## One harness, many workbenches

![An English launch brief open alongside a Sailry conversation](assets/readme/files.png)

Projects and conversations sit on the left, the current conversation in the
center, and files, previews, and tools alongside it. Plugins decide what that
workspace can do. Selected official plugins ship with the app for offline
installation and update independently of it, on local and remote Nodes alike.

| Workbench | Official plugins |
| --- | --- |
| Code | Files, Git, Worktrees, Commands and terminals, Code review, GitHub |
| Research and documents | Browser, Web search, Office documents, Media, Context7 |
| Servers and data | SSH, Databases |
| Agent workflow | Delegation, Goals, Progress, Memory, Scheduled tasks, Reminders, Usage statistics |
| Your computer | Computer control, External browsers |
| Downtime | Xiangqi, Gomoku, Reversi, Dou Dizhu, Texas Hold'em, Liar's Dice, City Trader |

Browse the [official plugins](https://github.com/sailry/sailry-plugins), or
build your own with the [Plugin SDK](sdk/plugins.md). Desktop plugin surfaces use
native GPUI Kit components, not a separate HTML interface. Skills and MCP are
supported too; they complement the plugin system rather than replace it.

![Sailry's official plugin marketplace in English](assets/readme/plugins.png)

## Choose providers, not a single ecosystem

The add-provider interface covers **14 provider brands and services**, counting
OpenCode Go and Zen as one service family.

| Connection | Built-in choices |
| --- | --- |
| Core model services | OpenAI, Anthropic, Google Gemini |
| 10 additional hosted providers | xAI, DeepSeek, Qwen, Moonshot (Kimi), Mistral, MiniMax, Doubao, Zhipu, Baidu, Cohere |
| Multi-model services | OpenCode Go and OpenCode Zen |
| Custom-compatible endpoints | OpenAI Responses, OpenAI Chat Completions, Anthropic Messages, Gemini generateContent |

Use your own keys, configure a compatible gateway or local model endpoint, or use
ChatGPT sign-in where offered. Model availability, reasoning, media, and tool
support depend on the provider and model you choose.

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

Desktop and Host do not bundle Python or Office script libraries. Office
inspection, previews, and PDF export use Rust. Document skills use the execution
Node's tools and prepare project-local dependencies only when a task needs them,
subject to the session's command permissions and network availability.

## Built on native foundations

Rust and GPUI Kit power the desktop; ADK-Rust powers agent execution. Execution,
transport, shared client state, and presentation have separate owners, so
Desktop, Host, and Mobile share one execution contract instead of each running
its own agent engine. See the [architecture](ARCHITECTURE.md) for the boundaries.

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
