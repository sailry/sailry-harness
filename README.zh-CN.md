<p align="center">
  <img src="assets/readme/hero.zh-CN.svg" width="100%" alt="Sailry：任何一台电脑都能成为你的 AI 工作站。桌面端和手机通过端到端加密的 P2P 链路连接 MacBook、Linux 服务器和 GPU 主机。">
</p>

<p align="center">
  <a href="README.md">English</a> ·
  <a href="#安装">安装</a> ·
  <a href="docs/hosts.md">主机</a> ·
  <a href="docs/models.md">模型</a> ·
  <a href="sdk/plugins.md">插件 SDK</a> ·
  <a href="https://discord.gg/MCDktmDey">Discord</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square" alt="Apache-2.0 许可证"></a>
  <a href="#安装"><img src="https://img.shields.io/badge/desktop-macOS-lightgrey?style=flat-square&logo=apple&logoColor=white" alt="桌面端支持 macOS"></a>
  <a href="#host"><img src="https://img.shields.io/badge/host-macOS%20·%20Linux-lightgrey?style=flat-square" alt="Host 支持 macOS 和 Linux"></a>
  <a href="#移动端"><img src="https://img.shields.io/badge/mobile-Android%20·%20iOS-lightgrey?style=flat-square" alt="移动端支持 Android 和 iOS"></a>
  <a href="https://github.com/sailry/sailry-harness/stargazers"><img src="https://img.shields.io/github/stars/sailry/sailry-harness?style=flat-square&color=yellow" alt="GitHub Stars"></a>
</p>

Sailry 是一个开源的 AI Harness——不只是智能体，而是直接运行在你的代码和数据所在机器上的原生工作区。装在任意一台 Mac 或 Linux 机器上，用桌面端或手机来驱动它，让智能体团队用[你选择的模型](docs/models.md)操作文件、终端、浏览器、Git、数据库和文档。

> [!NOTE]
> **早期预览版。** Sailry 版本之间变化较快。让智能体修改重要文件前请先备份。遇到问题欢迎[提交反馈](https://github.com/sailry/sailry-harness/issues)，你的反馈会直接影响后续开发。

<p align="center">
  <img src="assets/readme/workspace-dark.png" width="100%" alt="Sailry 桌面端暗色模式：左侧是主机和项目，中间是新会话。">
</p>

## 任何一台电脑，都能成为你的主机

在笔记本、实验室的 Linux 机器或租来的 GPU 机器上安装 Sailry Host。它作为后台服务运行，项目、终端和智能体都留在那台机器上，紧挨着它们需要的代码和数据。

在世界任何地方，用 Sailry 桌面端或手机连上它。能直连就走 P2P，直连不通就自动走加密中继——不用开端口、不用 VPN、不用云账号。合上笔记本，任务照样在 Host 上继续跑。

## 对标 ChatGPT 与 Claude Code，而且不止写代码

一个会话，一整套工具：

- **编程**：标准、计划、旗舰三种模式，`apply_patch` 多文件修改，Git 与 worktree、终端、代码审查。
- **浏览与操作**：智能体能阅读、点击、截图的内置浏览器；页面元素一键选进对话；桌面电脑操作。
- **办公**：读取 Word、Excel、PowerPoint 和 PDF（支持 OCR），撰写文档和可交互的 HTML 预览，生成图片和视频，搜索网络和知识库。
- **持续推进**：长期记忆、做完才停的目标、提醒和定时任务，以及 Notion、Figma、Google Workspace、Microsoft 365、钉钉、企业微信、语雀等连接器。

Sailry 更进一步的地方：

- **智能体团队，一个位置一个模型。** 旗舰模式把任务分给并行的子代理，每个子代理用自己的模型——DeepSeek 写代码、Claude 出方案、Codex 攻难题，每一步都在会话里看得见。
- **可检索的历史。** 智能体能找到并复用你之前的会话。
- **本地语音和 OCR。** 语音输入和文字识别都在你自己的机器上完成。

<p align="center">
  <img src="assets/readme/files.png" width="100%" alt="Sailry 会话把发布笔记整理成周计划，旁边是正在编辑的 Markdown 文档。">
</p>

## 不只是智能体，更是 AI Harness

Sailry 给模型套上了一层为工程实战打造的 harness。编码插件开箱即用，打开项目就能用，不用另装语言服务器、驱动或扩展：

- **代码图谱**：内置解析器为项目建立索引，支持符号搜索、文件大纲、调用方与被调用方、改动影响分析，以及按引用排序的项目地图，智能体直接读到该读的代码，不用全仓库盲搜。
- **数据库**：通过保存的连接访问 PostgreSQL、MySQL 和 SQLite。查询走只读连接；写操作遵循会话的权限策略。
- **SSH**：在保存的服务器上执行命令、通过 SFTP 传输文件，并固定校验主机密钥。

凭据只保存在执行任务的主机上。智能体通过连接 ID 使用连接，看不到密码和私钥；结构化工具只返回智能体需要的结果，而不是一大段原始 shell 输出。

## 手机就是你的遥控器

用六位配对码把 Android 或 iOS 应用连上桌面端或任意 Host。随时接手任何会话、发送和排队消息、处理审批、查看工具执行过程——无论你在哪里。连接是 P2P 端到端加密的，而且免费：不用账号，也没有订阅。

## 原生、轻量、够快

Sailry 用 Rust 编写，基于 GPUI 和 [GPUI Kit](https://github.com/dux-web/gpui-component) 在 GPU 上绘制界面——没有 Electron，也没有内置浏览器内核。启动快，长会话依然流畅，内存留给真正的工作，不发热、不卡顿。

## 一切皆可扩展

- **开放标准。** 插件包采用 [agent-plugins](https://agent-plugins.org) 格式和标准的 `SKILL.md` 技能，MCP 服务器可直接接入。
- **原生插件界面。** 插件的页面、面板和设置用 JavaScript 编写，Sailry 用原生 GPUI Kit 组件渲染，扩展的观感和性能与应用本身一致。
- **更多模型。** 通过模型插件添加更多供应商和账号。
- 浏览官方[插件](https://github.com/sailry/sailry-plugins)，或用 Apache-2.0 的 [SDK](sdk/plugins.md) 编写你自己的插件。

<p align="center">
  <img src="assets/readme/plugins.png" width="100%" alt="Sailry 插件市场：已安装和官方插件，包括浏览器、代码审查、电脑操作和数据库等。">
</p>

## 精致界面，你的主题

主题系统就是界面层本身：颜色、字体、表面和装饰都来自同一套设计令牌，支持亮色和暗色。在内置主题市场一键应用；官方主题以开放的主题包形式维护在 [sailry-themes](https://github.com/sailry/sailry-themes)。

## 安装

[更新日志](CHANGELOG.md) · [从源码构建](CONTRIBUTING.md#native-setup)

### 桌面端

| 系统 | 架构 | 状态 | 下载 |
| --- | --- | --- | --- |
| macOS 13+ | Apple 芯片 / Intel | 支持 | [Apple 芯片](https://github.com/sailry/sailry-harness/releases/latest/download/Sailry-aarch64-apple-darwin.dmg) · [Intel](https://github.com/sailry/sailry-harness/releases/latest/download/Sailry-x86_64-apple-darwin.dmg) |
| Windows · Linux | x64 / ARM64 | 计划中 | — |

### Host

在 macOS 或 Linux 机器上安装：

```sh
curl -fsSL https://github.com/sailry/sailry-harness/releases/latest/download/install-host.sh | bash
```

| 系统 | 架构 | 状态 | 下载 |
| --- | --- | --- | --- |
| macOS 13+ | Apple 芯片 / Intel | 支持 | [Apple 芯片](https://github.com/sailry/sailry-harness/releases/latest/download/sailry-host-aarch64-apple-darwin.tar.gz) · [Intel](https://github.com/sailry/sailry-harness/releases/latest/download/sailry-host-x86_64-apple-darwin.tar.gz) |
| Linux · glibc 2.34+ | x64 / ARM64 | 支持 | [x64](https://github.com/sailry/sailry-harness/releases/latest/download/sailry-host-x86_64-unknown-linux-gnu.tar.gz) · [ARM64](https://github.com/sailry/sailry-harness/releases/latest/download/sailry-host-aarch64-unknown-linux-gnu.tar.gz) |
| Windows | — | 计划中 | — |

### 移动端

| 系统 | 架构 | 状态 | 下载 |
| --- | --- | --- | --- |
| Android 7.0+ | ARM64 | 支持 | [APK](https://github.com/sailry/sailry-harness/releases/latest/download/Sailry-android-arm64.apk) |
| iOS 15+ | ARM64 | 测试版 | [TestFlight](https://testflight.apple.com/join/qaTnVrmQ) |

## 开始使用

1. 在**本机**或已连接的 Host 上，把现有文件夹或克隆的仓库添加为项目。
2. 选择**创建会话**，选定模型，描述你的任务。
3. [添加 Host 并配对设备](docs/hosts.md#install-and-pair)，随时随地继续工作。

使用指南（英文）：[模型配置](docs/models.md) · [远程主机](docs/hosts.md) · [项目与会话](docs/projects.md)

## 交流与合作

加入 [Discord](https://discord.gg/MCDktmDey)，或加我微信进交流群。Bug 反馈和功能请求请使用[GitHub Issues](https://github.com/sailry/sailry-harness/issues)，更容易跟踪。

<p align="center">
  <img src="assets/readme/wechat.png" width="200" alt="Sailry 交流群微信二维码">
</p>

## 许可证

Sailry 采用 [Apache-2.0](LICENSE) 许可证，[sdk/](sdk/) 中的插件 SDK、官方[插件](https://github.com/sailry/sailry-plugins)与[主题](https://github.com/sailry/sailry-themes)同样采用 Apache-2.0。许可证不包含 Sailry 名称和 Logo 的使用权，详见 [TRADEMARKS.md](TRADEMARKS.md)。第三方材料保留各自的许可证，详见 [NOTICE](NOTICE) 和[第三方许可声明](third_party_licenses/)。

[![贡献者](https://contrib.rocks/image?repo=sailry/sailry-harness)](https://github.com/sailry/sailry-harness/graphs/contributors)
