# 中继

桌面端、手机和 Host 之间点对点连接。两台设备无法直接互通时（例如分别在不同的 NAT 后面），由 libp2p Circuit Relay v2 中继服务器转发连接。流量在设备之间端到端加密，中继无法读取，也不会改变配对、信任关系或凭据。直连可用后，新的数据流会改走直连。

中继和配对服务是两回事：配对服务只把一分钟有效的六位配对码换成邀请，不承载应用流量。

## 选择中继

中继由发布地址的一端决定，通常是 Host。与它配对的设备沿用邀请里的中继，不会改变自己的选择。

在 Host 上运行 `sailry link`，选择网络：

- **直连（局域网）**：不使用中继。
- **具名中继**：来自[官方目录](https://sailry.dev/catalogs/network-presets-libp2p.json)。刷新失败时，Host 使用上次加载的目录。
- **自定义**：填写目录中的主机名或 IP，可带端口（`relay.example.com:4001`），或填写自建中继打印出的完整 libp2p multiaddr。

脚本中可以这样写：

```sh
sailry link --data-dir <profile> --relay <preset-id|direct|custom> [--address <中继地址>]
```

目录之外的主机名，可以在 `_dnsaddr.<主机名>` 发布 `dnsaddr=<带 PeerID 的 multiaddr>` 记录，这样也能用短地址。已保存的地址不依赖目录。`https://` 前缀只是输入简写，中继本身不走 HTTPS。

## 自建中继

需要一台有公网 IP 的 Linux 服务器，并开放一个 TCP 端口（默认 4001）。服务器前面有防火墙或 NAT 的话需要自己放行，中继不会自动开端口。

### 获取程序

每个[版本发布](https://github.com/sailry/sailry-relay/releases/latest)都附带 `sailry-relay-linux-amd64.tar.gz` 和 `sailry-relay-linux-arm64.tar.gz`，以及对应的 `SHA256SUMS-relay-linux-<架构>` 校验文件。按服务器架构下载并解压：

```sh
curl -fLO https://github.com/sailry/sailry-relay/releases/latest/download/sailry-relay-linux-amd64.tar.gz
tar -xzf sailry-relay-linux-amd64.tar.gz
```

压缩包内含 `sailry-relay` 和许可证。构建说明见[中继仓库](https://github.com/sailry/sailry-relay)。

### 配置

新建 `relay.toml`，把 `announce_ip` 换成设备访问这台服务器用的地址；`listen_ip` 用通配地址时必须填写它。

```toml
mode = "public"
listen_ip = "0.0.0.0"
port = 4001
announce_ip = "203.0.113.10"
data_dir = "/var/lib/sailry-libp2p-relay"
```

`data_dir` 保存中继的私有身份。重启和升级时请保持同一个绝对路径：中继地址由其中的密钥决定。

- **公开**模式在资源限额内为任何标准 libp2p 节点服务。
- **私有**模式只为列出的节点服务，连接的两端都必须在列表中：

  ```toml
  mode = "private"
  allowed_peers = ["12D3KooW...", "12D3KooW..."]
  ```

  `sailry-relay id --data-dir <profile>` 可以打印某个 Sailry 数据目录的 PeerID。

准入只看模式和 PeerID，没有访问令牌。预留数按 IP 和总量限制，每条中继连接也有时长和流量上限。

### 启动

```sh
./sailry-relay start --config ./relay.toml
```

`start` 在前台运行，并打印中继地址，例如 `/ip4/203.0.113.10/tcp/4001/p2p/12D3KooW...`。把这个地址填为**自定义**中继，或发布成 `dnsaddr` 记录以使用更短的名字。`--port` 只在本次运行中覆盖 TCP 端口，不改文件。

不用配置文件时，传入绝对路径的 `--data-dir`，再加 `--public` 或一个或多个 `--allow <PeerID>`。`--listen` 和 `--announce` 接受完整的 TCP 或 QUIC-v1 multiaddr。全部参数见 `sailry-relay --help`。

### 保持运行

中继只交给一个守护方式管理。

**systemd**：在解压后的目录中运行：

```sh
sudo ./sailry-relay daemon install --config ./relay.toml
```

它会安装 `/usr/local/bin/sailry-relay`、配置文件 `/etc/sailry-relay/relay.toml`，以及以专用用户运行、开机启动、退出后自动重启的 `sailry-relay` 服务。它不会接管已有服务，也不会覆盖已有配置。`data_dir` 的上级目录必须已存在、属于 root，且其他用户不可写。

修改配置时，直接编辑 `/etc/sailry-relay/relay.toml`，再运行 `sudo sailry-relay daemon restart`。`daemon start`、`stop`、`restart`、`status` 用于管理服务；`stop` 不会取消下次开机启动。

**宝塔等面板**：新建一个 Go 项目，可执行文件填 `sailry-relay` 的完整路径，参数填 `start --config <配置文件绝对路径>`，由面板负责重启和开机启动。此时不要再安装 systemd 服务。

### 更新

`sudo sailry-relay daemon update` 会校验 SHA-256 后安装更新的稳定版。离线时用 `sudo sailry-relay daemon update <程序> --sha256 <摘要>`，摘要是解压后可执行文件的，不是压缩包的。更新只替换程序，然后重启中继并确认 PeerID 不变；失败时恢复原来的程序。`relay.toml` 和身份文件不会被改动。

## 排查问题

- **设备仍然连不上**：确认外部能访问该 TCP 端口，且 `announce_ip` 是公网地址而不是内网地址。
- **中继地址变了**：`data_dir` 被替换或丢失。恢复该目录，或更新 Host 的自定义中继后重新配对。
- **私有中继拒绝设备**：连接两端都必须在 `allowed_peers` 中；改完文件后重启中继。

分享日志时，不要包含配对码、邀请票据和数据目录内容。
