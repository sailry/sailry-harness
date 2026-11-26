<p align="center">
  <img src="assets/branding/sailry.png" width="112" height="112" alt="Sailry 标志">
</p>

<h1 align="center">Sailry Harness</h1>

<p align="center">面向原生应用的开源 AI 智能体运行时与 SDK。</p>

<p align="center">
  <a href="#快速开始">快速开始</a> ·
  <a href="ARCHITECTURE.md">架构文档</a> ·
  <a href="sdk/plugins.md">插件 SDK</a> ·
  <a href="https://github.com/sailry/sailry-code">Sailry Code</a> ·
  <a href="README.md">English</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square" alt="Apache-2.0 许可证"></a>
  <a href="https://github.com/sailry/sailry-harness/stargazers"><img src="https://img.shields.io/github/stars/sailry/sailry-harness?style=flat-square&amp;color=yellow" alt="GitHub Stars"></a>
  <a href="https://discord.gg/MCDktmDey"><img src="https://img.shields.io/badge/chat-Discord-5865F2?style=flat-square&amp;logo=discord&amp;logoColor=white" alt="Discord"></a>
  <a href="https://awesomemygo.dev/apps/sailry"><img src="https://awesomemygo.dev/badge.svg" height="20" alt="已被 Awesome MyGo 收录"></a>
</p>

**Sailry Harness**（`github.com/sailry/sailry-harness`）提供共享的智能体运行时、桌面和移动端底座，以及 Go、Flutter 和插件公共 SDK。它也可以作为通用基础应用独立运行。

> [!TIP]
> 想使用 AI 研发工作站？请前往 [Sailry Code](https://github.com/sailry/sailry-code) 和[产品发布页](https://github.com/sailry/sailry-code/releases)。

## 架构概览

<p align="center">
  <img src="assets/readme/architecture.zh-CN.svg" width="100%" alt="桌面端与手机通过端到端加密的点对点链路连接 Host。Host 运行智能体、调用模型和工具，并在本机保存项目与凭据。">
</p>

- **[Node](internal/node/)** 运行智能体和工具、处理审批并保存凭据与会话历史。可内嵌于桌面端，也可作为无界面 Host 运行。

- **[Link v2](internal/link/)** 通过端到端加密的 libp2p 链路连接客户端与 Node，支持六位配对码和自动中继。

- **[Client](internal/client/)** 同步会话和工作区状态，支持断线恢复。桌面端使用 [MyGo](https://github.com/egoist/mygo)，移动端使用 [Flutter](apps/mobile/)。

组件归属、存储规则与模块依赖见[架构文档](ARCHITECTURE.md)。

## SDK 与扩展方式

下游产品使用 [`sdk/`](sdk/) 和 [`apps/mobile/`](apps/mobile/) 的公共包，不依赖 `internal/` 下的内部实现。

- **[Go 桌面 SDK](sdk/desktop/)**：装配原生桌面应用、页面、面板与设置。
- **[Go Node](sdk/native/node/) 与 [Host](sdk/host/)**：注册智能体工具、构建无界面 Host。
- **[Flutter 移动端](apps/mobile/)**：构建带产品专属界面的 Android 与 iOS 控制端。
- **[插件 SDK](sdk/plugins.md)**：通过 [agent-plugins](https://agent-plugins.org)、技能或 MCP 添加 JavaScript 界面和 Node 工具。

## 内置通用能力

基础应用使用可复用的[通用模块](modules/)，提供智能体工作模式、并行子智能体、沙箱命令和工作区文件搜索。

还支持浏览器与桌面自动化、Office/PDF 及本地 OCR、HTML 预览、媒体生成、网络搜索、知识库、记忆、定时任务与本地听写。新配置默认不预装插件或主题。

<a id="安装"></a>

<a id="开发快速上手"></a>

## 快速开始

### 作为 Go 库接入

按[环境配置指南](CONTRIBUTING.md#setup)安装 Go 1.27.3 和当前平台的前置依赖，创建自己的模块并引入 Harness。

```sh
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
mkdir my-agent
cd my-agent
go mod init example.com/my-agent
go get github.com/sailry/sailry-harness@main
```

`@main` 使用当前预览版 SDK，Go 会将解析出的版本记录到项目的 `go.mod`。

将下面的最小 Host 入口保存为 `main.go`。示例使用公共 SDK 和 Harness 内置模块装配。

```go
package main

import (
	"log"
	"os"

	"github.com/sailry/sailry-harness/sdk/host"
	"github.com/sailry/sailry-harness/sdk/native/node"
	"github.com/sailry/sailry-harness/sdk/product"
)

func main() {
	code, err := host.Run(os.Args[1:], host.Options{
		Identity: product.Desktop{
			ID:             "my-agent",
			Version:        "0.1.0",
			DefaultProfile: "~/.my-agent",
		},
		Assembly: node.HarnessAssembly(),
	})
	if err != nil {
		log.Fatal(err)
	}
	os.Exit(code)
}
```

```sh
go mod tidy
go run . --help
```

要启动 Host，先在 Harness 源码目录运行 `just native`，在 `.runtime/native/<goos>-<goarch>/` 准备原生 Turso 库。回到自己的项目，将环境变量设为该库的绝对路径：

```sh
export SAILRY_TURSO_LIB=/absolute/path/to/turso-library
go run . serve --data-dir "$PWD/.runtime/profiles/my-agent"
```

嵌入已有应用时，可通过 `host.RunContext` 控制退出。

原生桌面应用可使用 `sdk/desktop` 和可选的 `sdk/desktop/harness` 预设。公共扩展入口见 [SDK 概览](#sdk-与扩展方式)。

### 从源码运行

克隆仓库即可运行基础应用或参与 Harness 开发。

```sh
git clone https://github.com/sailry/sailry-harness.git
cd sailry-harness
just native
just desktop
```

- `just desktop dev` 使用独立的 `~/.sailry-harness-dev`，不使用默认的 `~/.sailry-harness`。
- `just host` 启动无界面 Host；`just check` 运行项目检查。
- 与 Code 联调时，在 Code 仓库的本地 `go.work` 中引用 Harness 工作树，不提交该文件。

Harness API 仍处于早期预览阶段。[Harness 历史版本](https://github.com/sailry/sailry-harness/releases)仅用于修复等场景，新的终端用户安装包在 [Sailry Code](https://github.com/sailry/sailry-code/releases) 发布。

## 下游产品与生态仓库

- **[Sailry Code](https://github.com/sailry/sailry-code)**：基于 Harness 的 AI 研发工作站，提供文件工作台、Git 与 worktree、终端、数据库和 SSH。
- **[Sailry Plugins](https://github.com/sailry/sailry-plugins)**：可选运行时插件。
- **[Sailry Themes](https://github.com/sailry/sailry-themes)**：官方亮色和暗色主题。

## 文档索引

- **开发文档**：[架构](ARCHITECTURE.md) · [贡献指南](CONTRIBUTING.md) · [工程规范](AGENTS.md) · [安全策略](SECURITY.md)
- **运行时指南**：[模型配置](docs/models.md) · [远程主机](docs/hosts.md) · [项目与会话](docs/projects.md) · [中继部署](docs/relays.zh-CN.md)

## 遥测、社区与许可证

### 隐私

程序默认发送匿名错误报告和功能计数，不包含会话、文件、凭据、路径或自定义名称。可在**设置 › 通用 › 诊断与隐私**中关闭，也可设置 `SAILRY_TELEMETRY=off` 或 `DO_NOT_TRACK=1`。公开模型评分源自 [ModelDial](third_party_licenses/modeldial.md)。

### 社区

[Discord](https://discord.gg/MCDktmDey) · [GitHub Issues](https://github.com/sailry/sailry-harness/issues)

<p align="center">
  <img src="assets/readme/wechat.png" width="160" alt="Sailry 交流群微信二维码">
</p>

### 许可证

[Apache-2.0](LICENSE) · [商标](TRADEMARKS.md) · [第三方声明](NOTICE) · [第三方许可证](third_party_licenses/)

[![贡献者](https://contrib.rocks/image?repo=sailry/sailry-harness)](https://github.com/sailry/sailry-harness/graphs/contributors)
