<p align="center">
  <img src="assets/branding/sailry-mark.svg" alt="Sailry Harness" width="88" />
</p>

<h1 align="center">Sailry Harness</h1>

<p align="center"><a href="README.md">English</a> · 简体中文</p>

<p align="center"><strong>把 AI 智能体、文件和工具放在同一个原生工作区。</strong></p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-blue?style=for-the-badge" alt="AGPL-3.0 License"></a>
  <a href="rust-toolchain.toml"><img src="https://img.shields.io/badge/Rust-1.98-orange?style=for-the-badge&logo=rust&logoColor=white" alt="Rust 1.98"></a>
  <a href="#安装"><img src="https://img.shields.io/badge/platform-macOS-lightgrey?style=for-the-badge&logo=apple&logoColor=white" alt="Platform macOS"></a>
  <a href="https://github.com/sailry/sailry-harness/stargazers"><img src="https://img.shields.io/github/stars/sailry/sailry-harness?style=for-the-badge&color=yellow" alt="GitHub Stars"></a>
  <a href="https://discord.gg/MCDktmDey"><img src="https://img.shields.io/badge/Discord-%E5%8A%A0%E5%85%A5-5865F2?style=for-the-badge&logo=discord&logoColor=white" alt="Discord"></a>
</p>

> [!NOTE]
> **早期预览版。** Sailry 仍在快速开发中，可能存在 bug，版本之间也会有较大变化。让智能体修改重要文件前请先备份。遇到问题欢迎[提交反馈](https://github.com/sailry/sailry-harness/issues)，你的反馈会直接影响后续开发。

![Sailry 暗色模式下的新会话与示例项目](assets/readme/workspace-dark.png)

使用自己的模型，在对话旁处理文件、Git、终端、浏览器、SSH 和数据库。
安装插件，让工作区适合自己的任务。
任务可在本机或自己的服务器上运行，用桌面端或手机端继续跟进。

## 为什么选择 Sailry？

Sailry 将模型、项目、SSH、数据库和插件整合进同一个原生桌面工作区。

对比包含产品官方提供的扩展。通过命令行或让智能体临时编写脚本操作数据库，
不等同于专门的数据库界面。

| 功能 | **Sailry** | [TRAE 中国版](https://docs.trae.cn/ide_traework-to-traecode-data-migration) | [WorkBuddy](https://www.codebuddy.cn/docs/workbuddy/Overview) | [Codex](https://learn.chatgpt.com/docs/environments/modes) | [Claude Code](https://code.claude.com/docs/en/overview) | [ZCode](https://zcode.z.ai/en/docs/welcome) | [DSH](https://github.com/deepseek-ai/deepseek-harness) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **数据库浏览、SQL 编辑与结果查看** | **✅** | — | — | — | — | — | — |
| **SSH 连接与远程操作** | **✅** | [✅](https://docs.trae.cn/enterprise_feature-list?lang=zh) | — | [✅](https://learn.chatgpt.com/docs/remote-connections) | [✅](https://code.claude.com/docs/en/desktop) | [✅](https://zcode.z.ai/en/docs/remote-development) | ◐ 执行适配 |
| **插件扩展工作区界面与设置** | **[✅](sdk/plugins.md)** | [◐ VSIX](https://docs.trae.cn/ide_manage-extensions) | [◐ 应用挂件](https://www.codebuddy.cn/docs/workbuddy/Changelog) | [◐ MCP 界面](https://learn.chatgpt.com/docs/plugins) | [◐ Mods](https://code.claude.com/docs/en/plugins/mods/overview) | [◐ 配置表单](https://zcode.z.ai/en/docs/plugin) | [✅](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/client/README.md) |
| **完整桌面应用与执行端源码开放** | **[✅](LICENSE)** | — | — | [◐ CLI／服务端](https://learn.chatgpt.com/docs/open-source) | [❌](https://github.com/anthropics/claude-code/blob/main/LICENSE.md) | — | [✅](https://github.com/deepseek-ai/deepseek-harness) |
| **主机互联与远程任务** | **✅ 桌面节点配对** | [✅ SSH](https://docs.trae.cn/ide_ssh-remote) | [◐ 手机连接电脑](https://www.codebuddy.cn/docs/workbuddyapp/features/Multidevice) | [✅ 设备互联／SSH](https://learn.chatgpt.com/docs/remote-connections) | [✅ SSH／Remote Control](https://code.claude.com/docs/en/desktop) | [✅ SSH／Remote](https://zcode.z.ai/en/docs/remote-development) | [◐ Web 服务](https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments) |
| **手机查看与控制任务** | **尚未发布** | [✅ App／即时通讯](https://forum.trae.cn/t/topic/15182) | [✅ App／小程序](https://www.codebuddy.cn/docs/workbuddyapp/features/Multidevice) | [✅ ChatGPT App](https://learn.chatgpt.com/docs/remote-connections) | [✅ App／浏览器](https://code.claude.com/docs/en/remote-control) | [✅ Remote／机器人](https://zcode.z.ai/en/docs/welcome) | [◐ 浏览器](https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments) |
| **独立部署自己的执行服务** | **安装包待发布** | — | — | [◐ App Server](https://learn.chatgpt.com/docs/app-server) | [◐ 远程执行](https://code.claude.com/docs/en/desktop) | [◐ 远程执行](https://zcode.z.ai/en/docs/remote-development) | [✅](https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments) |
| 自带 API Key／兼容模型网关 | **[✅](docs/models.md)** | [✅](https://docs.trae.cn/ide_models) | [✅](https://www.codebuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/Model) | [✅](https://learn.chatgpt.com/docs/config-file/config-advanced) | ✅ | [✅](https://zcode.z.ai/en/docs/configuration) | [✅](https://deepseek-harness.github.io/deepseek-harness/en/guide/providers) |
| Git Worktree 工作流 | **[✅](docs/projects.md)** | [✅](https://docs.trae.cn/enterprise_feature-list?lang=zh) | ✅ | ✅ | ✅ | — | [◐ 实验插件](https://github.com/deepseek-ai/deepseek-harness/releases) |

✅ 已确认支持 · ◐ 支持相关能力，范围或成熟度见单元格说明 · ❌ 已确认不支持 ·
— 尚未确认，不代表不支持。主机连接包含设备配对、SSH、Web 等不同方式；
手机控制指查看和管理电脑或服务器上的任务。模型接入须符合各产品支持的服务商与协议。
DSH 指 DeepSeek Harness，ZCode 指 Z.ai 面向 GLM 的开发工作区。

- **工具集中使用。** 在对话旁处理项目、终端、SSH 和数据库任务。
- **工作区可以扩展。** 插件通过公开的 [Apache-2.0 SDK](sdk/plugins.md)
  添加页面、面板、设置、智能体工具和技能。
- **模型与执行位置可选择。** 接入自己的模型服务、审阅源码，并配对桌面节点或连接
  [执行主机](docs/hosts.md)。独立 Host 安装包和手机控制的发布状态见下方。

Sailry 目前处于早期预览阶段：macOS 桌面端已提供，Host 安装包待发布，移动端尚未发布。
详见[平台支持情况](#安装)。

## 安装

[更新日志](CHANGELOG.md) · [反馈问题](https://github.com/sailry/sailry-harness/issues)

### 桌面端

| 系统 | 架构 | 状态 | 下载 |
| --- | --- | --- | --- |
| macOS 13+ | Apple 芯片 / Intel | 支持 | [预览版](https://github.com/sailry/sailry-harness/releases?q=prerelease%3Atrue) |
| Windows | x64 / ARM64 | 计划中 | — |
| Linux | x64 / ARM64 | 计划中 | — |

### Host

| 系统 | 架构 | 状态 | 下载 |
| --- | --- | --- | --- |
| macOS 13+ | Apple 芯片 / Intel | 待发布 | [预览版](https://github.com/sailry/sailry-harness/releases?q=prerelease%3Atrue) |
| Linux · glibc 2.34+ | x64 / ARM64 | 待发布 | [预览版](https://github.com/sailry/sailry-harness/releases?q=prerelease%3Atrue) |
| Windows | — | 计划中 | — |

### 移动端

| 系统 | 架构 | 状态 | 下载 |
| --- | --- | --- | --- |
| Android 7.0+ | ARM64 | 未发布 | — |
| iOS 15+ | ARM64 | 未发布 | — |

## 开始使用

在本机或已连接的 Host 上，将现有文件夹或克隆的仓库添加为项目。
选择**创建会话**，选定模型后描述任务；需要时确认工具操作。

## 使用指南（英文）

[模型配置](docs/models.md) · [远程主机](docs/hosts.md) · [项目与会话](docs/projects.md)

[从源码构建](CONTRIBUTING.md#native-setup)

## 交流与合作

有相关合作或想加入交流群，可以加我微信，或直接加入 Discord 社区：

<p align="center">
  <img src="assets/readme/wechat.png" width="240" alt="微信二维码">
  <br>
  <a href="https://discord.gg/MCDktmDey"><img src="https://img.shields.io/badge/Discord-%E5%8A%A0%E5%85%A5%E7%A4%BE%E5%8C%BA-5865F2?style=for-the-badge&logo=discord&logoColor=white" alt="加入 Sailry Discord 社区"></a>
</p>

> Bug 反馈和功能请求请使用 [GitHub Issues](https://github.com/sailry/sailry-harness/issues)，更容易跟踪。

## 许可证

Sailry 自有代码采用 [AGPL-3.0-only](LICENSE)。[sdk/](sdk/) 中的插件 SDK
以及 [NOTICE](NOTICE) 列出的插件接口文件采用 [Apache-2.0](sdk/LICENSE)，
官方[插件](https://github.com/sailry/sailry-plugins)与
[主题](https://github.com/sailry/sailry-themes)仓库同样采用 Apache-2.0。

无法遵守 AGPL 的使用场景可获取商业许可，请[联系维护者](https://github.com/sailry)。
详见 [COMMERCIAL.md](COMMERCIAL.md)。

第三方材料保留各自的许可证，详见 [NOTICE](NOTICE) 和[第三方许可声明](third_party_licenses/)。

## 贡献者

[![贡献者](https://contrib.rocks/image?repo=sailry/sailry-harness)](https://github.com/sailry/sailry-harness/graphs/contributors)
