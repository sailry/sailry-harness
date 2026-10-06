<p align="center">
  <img src="assets/branding/sailry-mark.svg" alt="Sailry Harness" width="88" />
</p>

<h1 align="center">Sailry Harness</h1>

<p align="center">
  English · <a href="README.zh-CN.md">Chinese (Simplified)</a>
</p>

<p align="center"><strong>A native AI workspace for coding and everyday work</strong></p>

<p align="center">
  Connect models, agents, and tools. Extend your workspace with plugins.
</p>

<p align="center">
  <a href="https://github.com/sailry/sailry-harness/releases">Releases</a> ·
  <a href="CHANGELOG.md">What's new</a> ·
  <a href="https://github.com/sailry/sailry-harness/issues">Feedback</a>
</p>

![Sailry workspace with an English conversation and weekly plan](assets/readme/workspace.png)

Sailry Harness is an **AI agent harness**: a native workspace for coding and
everyday work. It brings models, agents, context, tools, and permissions together,
with an interface to follow and steer the work.

Write and review code, research a topic, prepare documents, or manage project
tasks—with your own model providers and a local or remote computer doing the
work. Plugins extend this shared foundation into specialized workbenches with
their own panels, assistants, and tools.

## What makes Sailry Harness different

- **Lightweight native desktop, not Electron.** A Rust application built with GPUI Kit and GPU-rendered native controls, avoiding a browser-based application shell
- **Connected workspaces.** Pair trusted computers and return to the same execution workspace, conversations, and session configuration—not a copied chat history
- **Plugin-first extensibility.** Plugins can add tools, native panels, dedicated assistants, and workspace controls; skills and MCP complement that system
- **Your choice of models.** Built-in choices cover 14 provider brands and services, with custom endpoints for four common API protocols
- **Work beyond chat.** Keep files, document previews, a browser, terminals, Git, SSH, and database assistants close to your conversations
- **An execution layer you control.** Run agents locally or on your own Sailry Host, choose tool permissions, and keep provider credentials on the execution Node

These capabilities are designed to work together: a native desktop workspace,
a shared execution layer, and plugins that extend both the tools and the interface.

## Connect once, continue where you left off

Desktop includes a local execution Node. Connect another computer running Sailry
Host when the files, environment, or long-running work belong elsewhere. Pairing
establishes trust, and remote connections use authenticated, encrypted transport.

Reconnecting resumes the same Node-owned workspace and effective session
settings. Disconnecting the controller does not stop admitted work, provided the
execution Node stays running. Local and remote work use the same command model.

A Flutter mobile controller connects directly to the selected Node without using
Desktop as a gateway. Android is supported; iOS is in testing. Mobile controls
work on the connected Node rather than running a separate local agent engine.

## Install Desktop or Host

Download the macOS **DMG** for Apple silicon or Intel from
[Releases](https://github.com/sailry/sailry-harness/releases), then drag Sailry to
Applications. ZIP assets serve the desktop's automatic updater.

Linux Host is a separate release asset for **amd64** and **arm64**. It requires
systemd, glibc 2.28 or newer, and the X11/XInput client
libraries (`libX11.so.6` and `libXi.so.6`); no desktop session is needed for
headless tasks. On a minimal Debian or Ubuntu server, install those libraries
with `sudo apt-get install libx11-6 libxi6`. Install a published version without
building Rust:

```sh
curl -fsSL https://github.com/sailry/sailry-harness/releases/download/v0.1.0-alpha.1/install-host.sh | bash -s -- --version 0.1.0-alpha.1
```

The installer verifies the archive's release checksum and starts a persistent
service. Run it as the account that should own the Host; root uses a system
service, other accounts use a user service with lingering enabled. Enabling
lingering may require administrator access. Existing Host files and profiles are
not overwritten.

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

`share` asks the running Host for a private pairing PIN; it does not start another
Node. Keep the command running until pairing completes, or press Ctrl-C to cancel.
`update` downloads a published version, preserves `~/.sailry-host`, and restarts
the same service. Previous program files remain in a reported backup directory.
Without `--version`, installation and updates select the newest published
release, including previews. A failed startup leaves the profile unchanged.

For non-root installations, add `~/.local/bin` to PATH if needed:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Desktop's **Add Host** flow uses SSH to run the same official installer, pinned
to the desktop's release version, then automatically pairs with the service.
It verifies the installer and archive checksums; remote Host binaries are not
bundled inside Desktop. For an existing Host, use `sailry share` and pair instead
of installing again. Draft release assets are unavailable to this installer until
the release is published.

Desktop and Host do not bundle Python or Office script libraries. Office
inspection, previews and PDF export use Rust. Document skills use the execution
Node's tools and prepare project-local dependencies only when a task needs them,
subject to the session's command permissions and network availability.

## Start with what you need

| Make sense of things | Create something useful | Work on a project |
| --- | --- | --- |
| Summarize notes, explore questions, and organize next steps | Shape a draft, prepare a brief, or work with documents | Browse files, review changes, and run tools alongside your conversation |

## Keep your work in view

Your projects and conversations stay on the left. Your current conversation sits
in the center. Open files and previews alongside it, so you can keep the context
close while you work.

![An English launch brief open alongside a Sailry conversation](assets/readme/files.png)

## One harness, many workbenches

Use the same AI workspace across different kinds of work. Plugins can shape it
into a focused workbench for document workflows, database operations, or your
own domain—with a dedicated interface, an assistant, and the tools it needs.
The harness supplies shared conversations, execution, and permission controls;
plugins add the domain-specific capabilities.

Add capabilities from the official marketplace, or build your own. Plugins can
contribute agent tools, navigation entries, resource panels, embedded assistants,
composer actions, and context or statistics controls. Their desktop surfaces use
native GPUI Kit components—not a separate HTML interface.

Selected official plugins are bundled for offline installation. Installed plugins
can update independently of the application, through the same acquisition path
on local and remote Nodes. Skills supply reusable instructions; MCP connects
external tools. Neither replaces the richer workspace plugin system.

![Sailry's official plugin marketplace in English](assets/readme/plugins.png)

## Choose providers, not a single ecosystem

The current add-provider interface covers **14 provider brands and services**.
This counts OpenCode Go and Zen as one service family, not two vendors.

| Connection | Built-in choices |
| --- | --- |
| Core model services | OpenAI, Anthropic, Google Gemini |
| 10 additional hosted providers | xAI, DeepSeek, Qwen, Moonshot (Kimi), Mistral, MiniMax, Doubao, Zhipu, Baidu, Cohere |
| Multi-model services | OpenCode Go and OpenCode Zen |
| Custom-compatible endpoints | OpenAI Responses, OpenAI Chat Completions, Anthropic Messages, Gemini generateContent |

Use your own keys, configure a compatible gateway or local model endpoint, or use
ChatGPT sign-in where offered. DeepSeek has a dedicated adapter; OpenCode Go and
Zen route requests using the selected model's API. Model availability, reasoning,
media, and tool support depend on the provider and model you choose.

## Native foundations, modular by design

Rust and GPUI Kit power the desktop shell; ADK-Rust powers agent execution.
Execution, transport, shared client state, and presentation have separate owners,
so Desktop, Host, and Mobile share execution contracts rather than maintaining
separate agent engines. See the [architecture](ARCHITECTURE.md) for the boundaries.

Native rendering avoids running a browser-based shell for the whole application
interface. Embedded webviews are used for web content in browser panes, not to
render the application UI.

## Platforms

| Platform | Minimum requirements | Application | Status |
| --- | --- | --- | --- |
| macOS | macOS 13.0 Ventura or later; Apple silicon or Intel | Desktop workspace | Supported |
| Android | Android 7.0 or later (API 24); ARM64 | Mobile controller | Supported |
| Windows | System requirements under validation | Desktop workspace | In testing |
| iOS | iOS 15.0 or later | Mobile controller | In testing |

Mobile controllers connect to a computer running Sailry Desktop or Sailry Host.
Published packages are listed under [Releases](https://github.com/sailry/sailry-harness/releases);
platform support does not mean every platform already has a published installer.

## Get started

Sailry Harness is in active development. Check [Releases](https://github.com/sailry/sailry-harness/releases)
for available macOS builds, distributed outside the Mac App Store.

On macOS:

1. Choose the macOS package for your Mac: Apple silicon or Intel
2. Unzip it and drag **Sailry.app** into **Applications**
3. Open Sailry and connect a model provider
4. Add a folder or start a conversation

Screenshots show a fresh English-language demo workspace with illustrative
conversations, not personal data or previous test sessions.

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
