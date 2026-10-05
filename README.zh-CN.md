<p align="center">
  <img src="assets/branding/sailry-mark.svg" alt="Sailry" width="88" />
</p>

<h1 align="center">Sailry</h1>

<p align="center">
  <a href="README.md">English</a> · 简体中文
</p>

<p align="center"><strong>日常工作的 AI 工作区</strong></p>

<p align="center">
  对话、文件与工具，放在一起，推进你的工作。
</p>

<p align="center">
  <a href="https://github.com/sailry/sailry-harness/releases">下载版本</a> ·
  <a href="CHANGELOG.md">更新日志</a> ·
  <a href="https://github.com/sailry/sailry-harness/issues">反馈</a>
</p>

![Sailry 工作区中的英文对话与一周计划](assets/readme/workspace.png)

Sailry 把 AI 对话带到你的工作区。你可以从一个想法开始，把笔记整理成计划，
浏览文件，或与智能体一起推进项目——在对话与工具之间切换时，工作上下文始终相连。

## 从你的需求开始

| 理清思路 | 做出有用的内容 | 推进项目 |
| --- | --- | --- |
| 总结笔记、探索问题、整理下一步 | 打磨草稿、准备简报、处理文档 | 在对话旁浏览文件、审查改动、运行工具 |

## 让工作保持在眼前

项目和对话放在左侧，当前对话位于中间。文件与预览可以在旁边打开，
让你在工作时随时查看相关内容。

![Sailry 对话旁打开的英文发布简报](assets/readme/files.png)

## 按你的方式工作

- **选择模型。** 连接你偏好的模型服务，或兼容 OpenAI 的接口
- **带上文件。** 使用自己的文件夹，直接浏览和预览文件
- **添加工具。** 通过插件、技能和 MCP 连接扩展工作区
- **保持掌控。** 选择何时允许智能体编辑文件或运行工具
- **远程工作。** 在同一个桌面工作区中连接另一台电脑上的 Sailry Host
- **集中上下文。** 对话、终端、文件和项目工具放在同一个地方

![Sailry 的英文官方插件市场](assets/readme/plugins.png)

## 开始使用

Sailry 正在积极开发中。请查看[发布页面](https://github.com/sailry/sailry-harness/releases)，
获取可用的 macOS 版本；应用在 Mac App Store 之外分发。

1. 根据你的 Mac 选择安装包：Apple 芯片或 Intel
2. 解压后，将 **Sailry.app** 拖入 **Applications（应用程序）**
3. 打开 Sailry，连接模型服务
4. 添加文件夹，或开始一段对话

截图使用全新的英文示例工作区，其中的对话仅用于演示，不包含个人数据或之前的测试会话。

## 一起完善 Sailry

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

## 许可证

Sailry 自有代码采用 [Apache-2.0](LICENSE) 许可证。第三方源码、资源和 Shell 集成
保留各自的许可证与署名；请参阅 [NOTICE](NOTICE)、[源码声明](third_party_licenses/)
及 `vendor/` 中的声明。
引入的 Ghostty Shell 集成包含 GPL 材料，不会因 Sailry 的 Apache 许可证而变更许可。
