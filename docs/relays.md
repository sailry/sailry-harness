# Relays

Desktop, Mobile and Hosts connect peer to peer. When two devices cannot reach
each other directly, for example behind different NATs, a libp2p Circuit Relay v2
server forwards their connection. Traffic stays end-to-end encrypted between the
devices; the relay cannot read it and does not change pairing, trust or
credentials. Once a direct path works, new streams move to it.

The relay is separate from the pairing service, which only exchanges a six-digit,
one-minute code for an invitation and carries no application traffic.

## Choose a relay

Relay selection belongs to the Node whose address is published, usually the Host.
A device that pairs with it follows the relay in its invitation and keeps its own
selection.

On a Host, run `sailry link` and choose a network:

- **Direct (LAN)** uses no relay.
- A **named relay** comes from the
  [official directory](https://sailry.dev/catalogs/network-presets-libp2p.json).
  A Host uses the last directory it loaded if a refresh fails.
- **Custom** takes a hostname or IP from that directory, optionally with a port
  (`relay.example.com:4001`), or the full libp2p multiaddr printed by your own relay.

The scriptable form is:

```sh
sailry link --data-dir <profile> --relay <preset-id|direct|custom> [--address <relay address>]
```

A hostname outside the directory can publish `dnsaddr=<peer-addressed multiaddr>`
at `_dnsaddr.<hostname>` so that the short form resolves. Saved addresses do not
depend on the directory. An `https://` prefix is accepted as shorthand; the relay
itself does not use HTTPS.

## Run your own relay

You need a Linux server with a public IP address and one reachable TCP port
(4001 by default). Open that port in any firewall or NAT in front of the server;
the relay does not open ports itself.

### Get the program

Each [release](https://github.com/sailry/sailry-relay/releases/latest) includes
`sailry-relay-linux-amd64.tar.gz` and `sailry-relay-linux-arm64.tar.gz`, each with a
`SHA256SUMS-relay-linux-<arch>` file. Download the one for your server and extract it:

```sh
curl -fLO https://github.com/sailry/sailry-relay/releases/latest/download/sailry-relay-linux-amd64.tar.gz
tar -xzf sailry-relay-linux-amd64.tar.gz
```

The archive contains `sailry-relay` and its licenses. Build instructions are in
[the Relay repository](https://github.com/sailry/sailry-relay).

### Configure

Create `relay.toml`. Replace `announce_ip` with the address devices reach the
server at; a wildcard `listen_ip` requires one.

```toml
mode = "public"
listen_ip = "0.0.0.0"
port = 4001
announce_ip = "203.0.113.10"
data_dir = "/var/lib/sailry-libp2p-relay"
```

`data_dir` holds the relay's private identity. Keep the same absolute directory
across restarts and upgrades: its key determines the relay's address.

- **Public** mode serves any standard libp2p peer within its resource limits.
- **Private** mode serves only listed peers, and both ends of a connection must be
  listed:

  ```toml
  mode = "private"
  allowed_peers = ["12D3KooW...", "12D3KooW..."]
  ```

  `sailry-relay id --data-dir <profile>` prints the PeerID of a Sailry profile.

Admission is by mode and PeerID only; there are no access tokens. Reservations
are limited per IP and in total, and each relayed connection is limited in
duration and traffic.

### Start

```sh
./sailry-relay start --config ./relay.toml
```

`start` runs in the foreground and prints the relay's address, for example
`/ip4/203.0.113.10/tcp/4001/p2p/12D3KooW...`. Use that address as a **Custom**
relay, or publish it as a `dnsaddr` record for a shorter name. `--port` overrides
the TCP port for one run without editing the file.

Without a config file, pass an absolute `--data-dir` and either `--public` or one
or more `--allow <PeerID>`. `--listen` and `--announce` take full TCP or QUIC-v1
multiaddrs. Run `sailry-relay --help` for every option.

### Keep it running

Use one supervisor for the relay.

**systemd.** From the extracted package, run:

```sh
sudo ./sailry-relay daemon install --config ./relay.toml
```

It installs `/usr/local/bin/sailry-relay`, the config at
`/etc/sailry-relay/relay.toml` and a `sailry-relay` service that runs as a
dedicated user, starts at boot and restarts after an exit. It does not take over
an existing service or replace an existing config. The parents of `data_dir`
must already exist, be owned by root and not be writable by other users.

To change the configuration, edit `/etc/sailry-relay/relay.toml` and run
`sudo sailry-relay daemon restart`. `daemon start`, `stop`, `restart` and
`status` manage the service; `stop` does not disable it at the next boot.

**Panels such as BaoTa.** Create a Go project whose executable is the full path
of `sailry-relay`, with arguments `start --config <absolute config path>`, and
let the panel handle restarts and startup. Do not also install the systemd service.

### Update

`sudo sailry-relay daemon update` installs a
newer stable release after checking its SHA-256. Offline, use
`sudo sailry-relay daemon update <binary> --sha256 <digest>` with the digest of
the extracted executable. An update replaces only the program, restarts the
relay and confirms it still has the same PeerID; if that fails, the previous
program is restored. Your `relay.toml` and identity are not changed.

## Troubleshooting

- **Devices still cannot connect.** Check that the TCP port is reachable from
  outside and that `announce_ip` is the public address, not a private one.
- **The relay address changed.** The `data_dir` was replaced or lost. Restore it,
  or update the Host's custom relay and pair again.
- **Private relay refuses a device.** Both peers of the connection must be in
  `allowed_peers`; restart the relay after editing the file.

Keep pairing codes, invitation tickets and profile contents out of logs you share.
