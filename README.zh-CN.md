<p align="center">
  <img src="assets/branding/sailry-mark.svg" alt="Sailry Harness" width="88" />
</p>

<h1 align="center">Sailry Harness</h1>

<p align="center">
  <a href="README.md">English</a> · 简体中文
</p>

<p align="center"><strong>面向编码与日常工作的原生 AI 工作区</strong></p>

<p align="center">
  连接模型、智能体与工具，通过插件扩展工作台。
</p>

<p align="center">
  <a href="https://github.com/sailry/sailry-harness/releases">下载版本</a> ·
  <a href="CHANGELOG.md">更新日志</a> ·
  <a href="https://github.com/sailry/sailry-harness/issues">反馈</a>
</p>

![Sailry 工作区中的英文对话与一周计划](assets/readme/workspace.png)

Sailry Harness 是一个面向编码与日常工作的 **AI 工作区（AI Agent Harness）**。
这里的 Harness 将模型、智能体、上下文、工具和权限组织在一起，
提供可以查看工作过程、随时引导任务的原生界面。

你可以编写与审查代码、研究主题、准备文档或管理项目任务，使用自己的模型服务，
由本地或远程电脑执行工作。插件则在这套共享基础上扩展出各类专用工作台，
带来各自的面板、助手和工具。

## Sailry Harness 的特色

- **轻量原生桌面，非 Electron。** 使用 Rust 与 GPUI Kit 构建，通过 GPU 渲染原生控件，避免浏览器应用外壳的额外开销
- **工作区互联互通。** 配对可信设备，回到同一个执行工作区、对话和会话配置，而不是复制聊天记录
- **插件优先的扩展方式。** 插件可添加工具、原生面板、专属助手和工作区控件，技能与 MCP 则补充这一体系
- **模型选择权在你手中。** 内置选项覆盖 14 个供应商品牌及服务，并支持四种常用 API 协议的自定义接口
- **不止于聊天。** 文件、文档预览、浏览器、终端、Git、SSH 和数据库助手都可以放在对话旁
- **自己掌控执行环境。** 在本地或自己的 Sailry Host 上运行智能体，选择工具权限，模型凭据保留在执行节点

这些能力组合在一起，才是 Sailry Harness 的定位：原生桌面工作区、共享执行层，
以及同时扩展工具和界面的插件体系。

## 连接设备，继续原来的工作

桌面端内置本地执行节点。如果文件、环境或长期运行的任务在另一台电脑上，
可以连接运行 Sailry Host 的设备。配对建立信任，远程连接使用经过身份验证的加密传输。

重新连接后，继续使用同一节点拥有的工作区和实际会话配置。
只要执行节点仍在运行，控制端断开连接就不会停止已接收的工作。
本地与远程工作使用同一套命令模型。

Flutter 移动控制端直接连接选定节点，不需要桌面端充当网关。
Android 已支持，iOS 测试中。移动端控制所连接节点上的工作，
而不是在手机上运行另一套本地智能体引擎。

## 从你的需求开始

| 理清思路 | 做出有用的内容 | 推进项目 |
| --- | --- | --- |
| 总结笔记、探索问题、整理下一步 | 打磨草稿、准备简报、处理文档 | 在对话旁浏览文件、审查改动、运行工具 |

## 让工作保持在眼前

项目和对话放在左侧，当前对话位于中间。文件与预览可以在旁边打开，
让你在工作时随时查看相关内容。

![Sailry 对话旁打开的英文发布简报](assets/readme/files.png)

## 一个 Harness，多种工作台

同一个 AI 工作区可以适用于不同类型的工作。通过插件，可以扩展面向文档处理、
数据库操作或自定义业务的专用工作台，配备专属界面、助手和所需工具。
Harness 提供共享的对话、执行和权限控制，插件补充具体领域的能力。

从官方市场添加能力，或开发自己的插件。插件可以提供智能体工具、导航入口、
资源面板、嵌入式助手、输入区操作，以及上下文和统计控件。
桌面插件界面使用原生 GPUI Kit 组件，而不是另一套 HTML 界面。

部分官方插件随应用打包，可离线安装。已安装插件可以独立于应用更新，
本地与远程节点使用同一套获取流程。技能提供可复用的指令，MCP 连接外部工具；
它们都不替代更完整的工作区插件体系。

![Sailry 的英文官方插件市场](assets/readme/plugins.png)

## 自选供应商，不局限于单一生态

当前添加供应商的界面覆盖 **14 个供应商品牌及服务**。
这里将 OpenCode Go 与 Zen 计为同一个服务系列，而不是两家供应商。

| 连接类型 | 内置选项 |
| --- | --- |
| 核心模型服务 | OpenAI、Anthropic、Google Gemini |
| 10 个其他托管供应商 | xAI、DeepSeek、Qwen、Moonshot（Kimi）、Mistral、MiniMax、Doubao、Zhipu、Baidu、Cohere |
| 多模型服务 | OpenCode Go 与 OpenCode Zen |
| 自定义兼容接口 | OpenAI Responses、OpenAI Chat Completions、Anthropic Messages、Gemini generateContent |

可以使用自己的密钥，配置兼容网关或本地模型接口，也可以在提供该选项时使用
ChatGPT 登录。DeepSeek 使用专用适配器；OpenCode Go 与 Zen 根据所选模型的 API
路由请求。可用模型、推理、多媒体和工具能力取决于选择的供应商与模型。

## 原生基础，模块化架构

Rust 与 GPUI Kit 构成桌面界面，ADK-Rust 负责智能体执行。
执行、传输、共享客户端状态和界面各自有明确归属，因此 Desktop、Host 与 Mobile
共享执行契约，而不是各自维护一套智能体引擎。具体边界请查看[架构说明](ARCHITECTURE.md)。

原生渲染不需要为整个应用界面运行浏览器外壳。
内嵌 WebView 用于浏览器面板中的网页内容，不负责渲染应用界面。

## 支持的平台

| 平台 | 最低要求 | 应用形态 | 状态 |
| --- | --- | --- | --- |
| macOS | macOS 13.0 Ventura 及以上；Apple 芯片或 Intel | 桌面工作区 | 支持 |
| Android | Android 7.0 及以上（API 24）；ARM64 | 移动控制端 | 支持 |
| Windows | 系统要求验证中 | 桌面工作区 | 测试中 |
| iOS | iOS 15.0 及以上 | 移动控制端 | 测试中 |

移动控制端需要连接运行 Sailry Desktop 或 Sailry Host 的电脑。
已发布的安装包请查看[发布页面](https://github.com/sailry/sailry-harness/releases)；
平台支持不表示每个平台都已经提供公开安装包。

## 开始使用

Sailry Harness 正在积极开发中。请查看[发布页面](https://github.com/sailry/sailry-harness/releases)，
获取可用的 macOS 版本；应用在 Mac App Store 之外分发。

macOS 安装步骤：

1. 根据你的 Mac 选择安装包：Apple 芯片或 Intel
2. 解压后，将 **Sailry.app** 拖入 **Applications（应用程序）**
3. 打开 Sailry，连接模型服务
4. 添加文件夹，或开始一段对话

截图使用全新的英文示例工作区，其中的对话仅用于演示，不包含个人数据或之前的测试会话。

## 一起完善 Sailry Harness

发现问题或有新的想法？欢迎[提交 Issue](https://github.com/sailry/sailry-harness/issues)。
如果希望修改应用，请先阅读[贡献指南](CONTRIBUTING.md)。

<details>
<summary>面向贡献者与插件作者</summary>

- [本地开发环境与检查](CONTRIBUTING.md#native-setup)
- [架构](ARCHITECTURE.md)
- [工程规范](AGENTS.md)
- [插件 SDK](sdk/plugins.md)
- [官方插件](https://github.com/sailry/sailry-plugins)
- [Host 与配对](services/pairing-relay/README.md)

</details>

## 致谢

感谢让 Sailry Harness 成为可能的开源项目及其贡献者，特别是：

- [GPUI Kit](https://github.com/longbridge/gpui-kit)（Longbridge）——桌面组件、主题和原生插件界面
- [GPUI](https://gpui.rs/)（Zed 团队）——GPU 加速的桌面渲染
- [ADK-Rust](https://github.com/zavora-ai/adk-rust)——智能体执行和模型集成
- [iroh](https://github.com/n0-computer/iroh)——设备间的加密连接
- [Ghostty](https://github.com/ghostty-org/ghostty)——内嵌终端渲染器和 Shell 集成
- [Wry](https://github.com/tauri-apps/wry) 与 GPUI Kit 的 WebView 集成——内嵌浏览器面板
- [rquickjs](https://github.com/DelSkayn/rquickjs) 与 [QuickJS](https://bellard.org/quickjs/)——无界面插件回调
- [Flutter](https://github.com/flutter/flutter)——移动控制端

也感谢这里使用的其他库、字体、图标和文档工具的维护者。
署名和许可证声明保留在 [NOTICE](NOTICE)、[第三方声明](third_party_licenses/)
及 `vendor/` 中。

## 许可证

Sailry 自有代码采用 [Apache-2.0](LICENSE) 许可证。第三方源码、资源和 Shell 集成
保留各自的许可证与署名；请参阅 [NOTICE](NOTICE)、[源码声明](third_party_licenses/)
及 `vendor/` 中的声明。
引入的 Ghostty Shell 集成包含 GPL 材料，不会因 Sailry 的 Apache 许可证而变更许可。
