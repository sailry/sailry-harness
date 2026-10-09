<p align="center">
  <img src="assets/branding/sailry-mark.svg" alt="Sailry Harness" width="88" />
</p>

<h1 align="center">Sailry Harness</h1>

<p align="center">
  <a href="README.md">English</a> · 简体中文
</p>

<p align="center"><strong>原生、轻量、好看、可扩展的 AI 工作区</strong></p>

<p align="center">
  模型和工具由你选择，工作区也可以是你喜欢的样子。
</p>

<p align="center">
  <a href="https://github.com/sailry/sailry-harness/releases">下载版本</a> ·
  <a href="CHANGELOG.md">更新日志</a> ·
  <a href="https://github.com/sailry/sailry-harness/issues">反馈</a>
</p>

> [!NOTE]
> Sailry Harness 仍处于早期开发阶段，迭代较快。功能、设置和插件接口可能随版本调整，
> 部分功能尚未完善。更新前建议先查看[更新日志](CHANGELOG.md)，遇到问题欢迎[反馈](https://github.com/sailry/sailry-harness/issues)。

![Sailry 工作区中的英文对话与一周计划](assets/readme/workspace.png)

Sailry Harness 把 **AI 智能体和实际工作的工具放进同一个原生工作区**。
选择模型，连接项目，就能和智能体一起工作：在对话旁查看文件、审阅差异、使用终端、
浏览资料或查询数据库。

连接你喜欢的模型，用插件和主题打造自己的工作区。
任务可以在自己的电脑或服务器上执行，用桌面端与手机继续跟进。

## 原生渲染，轻量设计，高颜值，自由扩展

### 从工作区到插件，原生渲染

桌面界面与插件面板使用 **Rust 和 GPU 渲染的 GPUI Kit 控件**，
共享主题、焦点与键盘交互。内嵌终端由 Ghostty 驱动，网页内容和文档预览使用各自的内嵌 WebView。

文件、差异、终端与对话都在应用中一起使用。插件界面与工作区使用同一套原生组件。

### 核心精简，工具按需选择

Rust 负责执行、连接和共享状态，具体工作台通过插件提供。
安装当前工作需要的工具，再按需求扩展。部分插件随应用打包，支持离线安装和独立更新。

桌面端内置本地执行节点，Host 将同一套引擎作为服务运行，移动端保留共享客户端与控制界面。
切换控制设备时，任务继续留在你选择的执行机器上。

### 好看，也适合每天使用

左侧是项目和对话，中间是智能体，右侧是文件、预览和工具。
对话与结果同时可见：一边讨论一边审阅改动，在数据库助手旁查看查询结果。

选择浅色或深色模式、跟随系统外观，或导入[主题包](themes/README.md)。
主题可以定制配色、背景和装饰细节，原生控件与插件面板共享应用主题。

![Sailry 对话旁打开的英文发布简报](assets/readme/files.png)

### 扩展工具，也扩展界面

插件可以提供**工具、导航、资源面板、嵌入式助手和输入区操作**。
文件、Git、终端、SSH 和数据库也使用同一套扩展体系。
构建数据库工作台、服务器助手或专用工作流时，可以同时提供智能体工具和原生操作界面。

技能提供可复用工作流，MCP 连接工具服务，Sailry 插件还能塑造使用这些工具的工作区。
从[官方插件](https://github.com/sailry/sailry-plugins)开始，或用[插件 SDK](sdk/plugins.md)开发自己的扩展。

桌面端、Host、移动端和官方插件源码采用 Apache-2.0 许可证，第三方许可保留。

## 可以用它完成什么

| 你的任务 | Sailry 中的工作方式 |
| --- | --- |
| 重构代码仓库 | 在智能体旁使用项目文件、Git 差异、工作树、终端和代码审查 |
| 研究并准备简报 | 在同一工作区浏览网页、搜索资料、查看来源文件和文档预览 |
| 排查服务器或数据库 | 使用 SSH、数据库资源面板，以及关联到对应连接的助手 |
| 让长期任务在服务器上执行 | 从桌面端在 Host 上启动任务，再用桌面端或手机查看进度、引导工作 |
| 构建专用助手 | 用插件组合自己的工具、原生面板、嵌入式对话和节点存储 |

## 任务进行中，也能掌控方向

- **规划与审阅。** 在计划模式中探索，审阅方案后明确切换到执行修改
- **引导运行中的任务。** 执行期间发送引导，在当前这批工具调用结束后生效，对话持续保留
- **带着上下文委派。** 子代理有各自的角色和模型，保留历史供后续任务使用，也可以在后台运行。
  主智能体负责引导和等待，子代理审批出现在主对话中
- **选择权限。** 按会话配置工具访问权限，规划、权限与执行分别管理

## 同一个任务，多个控制端

使用桌面端内置的节点在本地执行，也可以连接**自己服务器上的 Sailry Host**。
桌面端的 **添加主机** 通过 SSH 安装服务并自动配对。
让长期任务留在 Host 上，关闭桌面控制端，再用桌面端或手机回到同一个对话。

移动端通过**端到端加密连接**直接访问已配对节点，无需 Sailry 账号或桌面端网关。
节点需要持续运行且保持唤醒：本地节点随桌面应用运行，Host 则是独立服务。

```text
          ┌─ 进程内 ─── 桌面端内置的本地节点
Desktop ──┤
          └─ 加密连接 ─┐
                       ├── 你服务器上的 Sailry Host
Mobile ──── 加密连接 ──┤
                       └── 已配对桌面端的本地节点
```

使用 60 秒内有效的六位配对码建立信任，已配对设备无需再次输入。
配对服务交换邀请凭证，对话经节点的加密连接传输。
无法直连时，可由数据中继转发加密流量。

重新连接后，回到节点保存的对话和任务状态。控制端断开不会取消已接收的工作，
停止任务需要明确操作。结果不确定的副作用会如实报告，不会自动重放。
执行、恢复与数据边界见[架构说明](ARCHITECTURE.md)。

## 用插件组成自己的工作区

| 工作 | 官方插件 |
| --- | --- |
| 编码 | 文件、Git、工作树、命令与终端、代码审查、GitHub |
| 研究与文档 | 浏览器、网页搜索、Office 文档、多媒体、Context7 |
| 服务器与数据 | SSH、数据库 |
| 智能体工作流 | 委派、目标、进度、记忆、定时任务、提醒、使用统计 |
| 你的电脑 | 电脑控制、外部浏览器 |
| 休闲 | 中国象棋、五子棋、黑白棋、斗地主、德州扑克、吹牛骰子、城市大亨 |

![Sailry 的英文官方插件市场](assets/readme/plugins.png)

具体工具的可用性取决于所选节点、控制端、平台与权限。

## 连接你的模型

| 连接类型 | 内置选项 |
| --- | --- |
| 核心模型服务 | OpenAI、Anthropic、Google Gemini |
| 其他托管供应商 | xAI、DeepSeek、Qwen、Moonshot（Kimi）、Mistral、MiniMax、Doubao、Zhipu、Baidu、Cohere |
| 多模型服务 | OpenCode Go 与 OpenCode Zen |
| 云服务集成 | Azure OpenAI |
| 自定义兼容接口 | OpenAI Responses、OpenAI Chat Completions、Anthropic Messages、Gemini generateContent |

使用自己的密钥、兼容网关、本地接口，或在提供该选项时使用 ChatGPT 登录。
为子代理角色配置不同的模型和指令，分别用于探索、实现与审查。

供应商凭据保存在所选节点的受保护存储中。请求发送给你选择的供应商，本地接口可用于本地推理。
可用模型、推理、多媒体和工具能力取决于供应商与模型。

## 支持的平台

| 平台 | 最低要求 | 应用形态 | 状态 |
| --- | --- | --- | --- |
| macOS | macOS 13.0 Ventura 及以上；Apple 芯片或 Intel | 桌面工作区 | 支持 |
| Linux | amd64 或 arm64；systemd 与 glibc 2.28 及以上 | Sailry Host 服务 | 支持 |
| Android | Android 7.0 及以上（API 24）；ARM64 | 移动控制端 | 支持 |
| Windows | 系统要求验证中 | 桌面工作区 | 测试中 |
| iOS | iOS 15.0 及以上 | 移动控制端 | 测试中 |

移动控制端需要连接运行 Sailry Desktop 或 Sailry Host 的电脑。
平台支持不表示每个平台都已经提供公开安装包，请查看[发布页面](https://github.com/sailry/sailry-harness/releases)。

## 开始使用

Sailry Harness 正在积极开发中。macOS 版本通过[发布页面](https://github.com/sailry/sailry-harness/releases)分发，
不经过 Mac App Store。

1. 根据你的 Mac 下载 DMG：Apple 芯片或 Intel
2. 打开后，将 **Sailry.app** 拖入 **Applications（应用程序）**
3. 打开 Sailry，连接模型服务
4. 添加文件夹，或开始一段对话

同一发布中的 ZIP 文件供桌面端自动更新使用。

截图使用全新的英文示例工作区，其中的对话仅用于演示，不包含个人数据或之前的测试会话。

### 安装 Sailry Host

桌面端的 **添加主机** 通过 SSH 安装并配对。手动安装方式：

<details>
<summary>Linux 要求、安装与服务命令</summary>

Linux Host 是单独的发布文件，支持 **amd64** 与 **arm64**。它需要 systemd、
glibc 2.28 及以上，以及 X11/XInput 客户端库（`libX11.so.6` 与 `libXi.so.6`）；
无界面任务不需要桌面会话。在精简的 Debian 或 Ubuntu 服务器上：

```sh
sudo apt-get install libx11-6 libxi6
curl -fsSL https://github.com/sailry/sailry-harness/releases/download/v0.1.0-alpha.1/install-host.sh | bash -s -- --version 0.1.0-alpha.1
```

安装程序会校验发布文件的校验和，并启动常驻服务。请使用应拥有该 Host 的账户运行：
root 使用系统服务，其他账户使用启用了 lingering 的用户服务，启用时可能需要管理员权限。
已有的 Host 文件和配置不会被覆盖。草稿版本的发布文件在正式发布前无法通过安装程序获取。

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

`share` 向正在运行的 Host 申请私有配对 PIN；配对完成前请保持命令运行，按 Ctrl-C 可取消。
`update` 下载已发布的版本，保留 `~/.sailry-host` 并重启同一个服务，
旧的程序文件会保存在提示的备份目录中。不指定 `--version` 时，安装和更新会选择最新发布的版本，
包括预览版。启动失败不会修改配置。非 root 安装如有需要，请将 `~/.local/bin` 加入 PATH：

```sh
export PATH="$HOME/.local/bin:$PATH"
```

桌面端的 **添加主机** 会通过 SSH 运行同一个官方安装程序，版本固定为桌面端的发布版本，
校验安装程序与压缩包的校验和后再与服务配对。已有 Host 时，请运行 `sailry share` 配对，
无需重新安装。

</details>

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
