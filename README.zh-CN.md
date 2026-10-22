<p align="center">
  <img src="assets/branding/sailry-mark.svg" alt="Sailry Harness" width="88" />
</p>

<h1 align="center">Sailry Harness</h1>

<p align="center"><a href="README.md">English</a> · 简体中文</p>

<p align="center"><strong>把 AI 智能体、文件和工具放在同一个原生工作区。</strong></p>

> [!NOTE]
> **早期预览版。** Sailry 仍在快速开发中，可能存在 bug，版本之间也会有较大变化。让智能体修改重要文件前请先备份。遇到问题欢迎[提交反馈](https://github.com/sailry/sailry-harness/issues)，你的反馈会直接影响后续开发。

![Sailry 暗色模式下的新会话与示例项目](assets/readme/workspace-dark.png)

使用自己的模型，在对话旁处理文件、Git、终端、浏览器、SSH 和数据库。
安装插件，让工作区适合自己的任务。
任务可在本机或自己的服务器上运行，用桌面端或手机端继续跟进。

## 为什么选择 Sailry？

- **为每个会话选择模型。** 接入自己的模型服务账号或兼容的 API 地址，
  [切换模型](docs/models.md)后继续使用同一项目和工具。
- **扩展整个工作区。** 插件可以通过公开的 [Apache-2.0 SDK](sdk/plugins.md)
  添加页面、面板、设置、智能体工具和技能。
- **自主选择任务运行的位置。** 桌面端自带本地执行能力，独立的
  [Host](docs/hosts.md) 在自己的机器上使用同一套运行时。
  控制端断开连接后，已接收的任务仍会继续执行。

这些产品的能力有所重叠，但工作方式和扩展机制各有侧重。表中链接均指向官方文档。

| 产品 | 工作方式 | 自定义与扩展 |
| --- | --- | --- |
| **Sailry** | 将会话、项目和工具放在同一个原生桌面工作区 | 自定义模型接口；界面、工具和技能插件；MCP |
| [TRAE（中国版）](https://docs.trae.cn/ide_traework-to-traecode-data-migration) | 集成 Agent 与 IDE 的软件开发工作区 | [内置及自定义模型](https://docs.trae.cn/ide_models)；技能、MCP 和插件 |
| [WorkBuddy](https://www.codebuddy.cn/docs/workbuddy/Overview) | 面向办公、编程和内容处理的桌面任务助手 | [自定义模型，包括 Ollama](https://www.codebuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Function-Description/Model)；技能与 [MCP/CLI 连接器](https://open.workbuddy.cn/docs/connector) |
| [Codex](https://learn.chatgpt.com/docs/environments/modes) | 在本地项目、工作树和云端环境中编程 | [自定义服务商、模型网关和本地模型](https://learn.chatgpt.com/docs/config-file/config-advanced)；[技能、MCP 和插件](https://learn.chatgpt.com/docs/skills-and-plugins) |
| [Claude Code](https://code.claude.com/docs/en/overview) | 通过终端、IDE、桌面端或浏览器编程 | 技能、MCP 和插件；受支持的模型服务与网关部署 |
| [ZCode](https://zcode.z.ai/en/docs/welcome) | 为 GLM 优化的开发工作区 | [第三方模型服务](https://zcode.z.ai/en/docs/configuration)；技能、MCP、钩子和[插件](https://zcode.z.ai/en/docs/plugin) |
| [DeepSeek Harness（DSH）](https://www.deepseek.com/en/harness/) | 面向编程与日常任务的桌面及网页工作区 | [自定义模型服务](https://deepseek-harness.github.io/deepseek-harness/en/guide/providers)；模型、工具、技能和界面插件 |

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
