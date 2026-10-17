<p align="center">
  <img src="assets/branding/sailry-mark.svg" alt="Sailry Harness" width="88" />
</p>

<h1 align="center">Sailry Harness</h1>

<p align="center">English · <a href="README.zh-CN.md">Chinese (Simplified)</a></p>

<p align="center">AI agents, files and tools in one native workspace.</p>

<p align="center">
  <a href="https://github.com/sailry/sailry-harness/releases">Download</a> ·
  <a href="CHANGELOG.md">Changelog</a> ·
  <a href="https://github.com/sailry/sailry-harness/issues">Feedback</a>
</p>

> Sailry is in early development. Features and plugin APIs may change between releases.

![Sailry workspace with an English conversation and weekly plan](assets/readme/workspace.png)

## What you can do

- Work with agents beside files, Git diffs, terminals, browsers, SSH and databases
- Connect your own model providers, compatible gateways or local endpoints;
  use account sign-in where available
- Plan, review, delegate and guide tasks with explicit tool permissions
- Run tasks locally in Desktop or on your own Host, then reconnect from Desktop
  or Mobile over an encrypted link. The execution Node must remain running
- Extend tools and native panels with [plugins](https://github.com/sailry/sailry-plugins),
  [skills and MCP](sdk/plugins.md), and [themes](https://github.com/sailry/sailry-themes)

Desktop uses Rust and GPUI Kit. Host runs the same execution engine as a service;
Mobile is a controller. Models, tools and permissions follow the selected Node.

## Get started

1. Download the DMG for your Mac from [Releases](https://github.com/sailry/sailry-harness/releases)
2. Drag **Sailry.app** into **Applications** and open it
3. Connect a model provider, then add a folder or start a conversation

See each release's assets for available builds. Desktop publishes first; Host
packages follow after packaging succeeds. Mobile distribution remains manual.
Screenshots show an English demo workspace with illustrative content.

## Platforms

| Platform | Requirements | Application |
| --- | --- | --- |
| macOS | macOS 13.0+; Apple silicon or Intel | Desktop and Host |
| Linux | amd64 or arm64; systemd and glibc 2.28+ | Host |
| Android | Android 7.0+ (API 24); ARM64 | Mobile controller |
| Windows | Requirements under validation | Desktop, in testing |
| iOS | iOS 15.0+ | Mobile controller, in testing |

### Install Sailry Host

Use Desktop's **Add Host** to install and pair over SSH. To pair with an existing
Host, run `sailry share` on that machine and enter its PIN in Desktop or Mobile.

Linux requires the X11/XInput client libraries. For Debian or Ubuntu:

```sh
sudo apt-get install libx11-6 libxi6
curl -fsSL https://github.com/sailry/sailry-harness/releases/download/v0.1.0-alpha.1/install-host.sh | bash -s -- --version 0.1.0-alpha.1
export PATH="$HOME/.local/bin:$PATH"
sailry share
```

Run the installer as the account that should own the Host. It verifies release
checksums and starts a persistent service without overwriting existing profiles.
Root installs a system service; other accounts use a user service with lingering,
which may require administrator access.

Use `sailry help` for service controls. For noninteractive tasks, CI and ACP editor
connections, see [Host automation](CONTRIBUTING.md#host-automation).

## Develop and extend

- [Contributing and building from source](CONTRIBUTING.md)
- [Architecture and data boundaries](ARCHITECTURE.md)
- [Plugin SDK](sdk/plugins.md) and [official plugins](https://github.com/sailry/sailry-plugins)
- [Theme packages](https://github.com/sailry/sailry-themes)
- [Pairing service](services/pairing-relay/README.md) and [engineering rules](AGENTS.md)

Thanks to GPUI Kit/GPUI, ADK-Rust, Ghostty, iroh, Flutter and the other projects
credited in [NOTICE](NOTICE) and [third-party notices](third_party_licenses/).

## License

Sailry-owned code is licensed under [Apache-2.0](LICENSE). Third-party materials
retain their own licenses and attribution; see [NOTICE](NOTICE),
[third-party notices](third_party_licenses/) and notices in `vendor/`.
Imported Ghostty shell integration includes GPL material and is not relicensed
by Sailry's Apache license.

For optional paid services and separate agreements, see [Commercial cooperation](COMMERCIAL.md).
