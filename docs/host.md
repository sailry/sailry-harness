# Host

A host's page: its overview in the main column and its metrics in the
right column, with the update dialog, the tunnel policy editor and the
port forwards (sailry-public `workspace/host.rs`, `workspace/host/{
activity,metrics,metrics/data,runtime,tunnels,tunnels/editor,update}.rs`,
`workspace/model/{runtime,tunnels,updates}.rs`, the header of
`live/host.rs` and `ports/{menu,picker,view}.rs`).

## Package and files

`internal/host`

| File | Holds |
|---|---|
| `model.go` | **The view model**: `Host`, `Project`, `Activity`, `Runtime` (`Facts`, `UptimeParts`, `Protocol`), `Tunnels` (`SavePorts`, `Close`, `PortsLabel`), `Update` (`Phase`, `Primary`, `Advance`, `Headline`), `Metrics` (`Bytes`, `Percent`, `Rate`, `Usage`), `Port` (`AddPort`) |
| `page.go` | `State`, `Actions`, `Page`: header, facts, projects, runtime, activity; `Platform`; failure toasts |
| `tunnels.go` | The tunnels section and the policy editor |
| `update.go` | The update dialog, `OpenUpdate` |
| `metrics.go` | `MetricsPanel`, the right column |
| `ports.go` | The header's forwards, the port picker and the new forward dialog |

Preview data: `preview.HostPages(d)` (`internal/preview/hosts.go`), one
`Host` per `Data.Hosts` entry: projects, counts and running sessions from
the dataset, this host's runtime and editable tunnel policy, the paired
host's unknown runtime, read-only policy and update, the metrics sample of
`Snapshot::sample` and two forwards.

## Entry point

The rail's host menu (`shell.App.hostPicker`): choosing a host sets
`App.Host` and opens `HostPage` (`Shell::select_host`); the host's Update
opens the page with its update dialog; the right-click menu's Host opens
the current host's page. Add host opens Settings › Connections with the
Add host dialog (`settings.State.AddHost`). The header shows the host's
name; the details button and ⌥⌘B toggle the metrics (`App.Metrics`, the
reference's per-page panel), ⌘W in the column closes them, ⌘W in the page
closes the page.

```go
h, st := a.hostPage()                // preview.HostPages, kept per host
host.Page(c, st, h, host.Actions{...}) // main column
host.MetricsPanel(c, st, h)          // right column
```

## Layout

The overview is 800 wide at most, centered, padded 24 with 24 between
sections: the platform logo (40), name (20 semibold), `workspace_host_status`
and the platform line; the first three forwards and Port forwarding, then
Update host on a host updated from here. A one-column description list
(type, projects, worktrees, sessions, terminals, active sessions), the
projects (rows opening the project's first conversation, or the dashed
empty card with Add project), the runtime's five facts with
`host_unknown` for unknown ones, the running sessions (agent or waiting
user, title, project / branch, model, status; a click opens the session)
and the tunnels (policy with Edit on this host, read-only note on another,
each tunnel with Open and Close).

The metrics column: a 40 DIP header with the chart icon, the preview note,
the host, CPU and memory with its bar, network rates, disks and the
busiest processes, an empty card where there are none, an indeterminate
bar until the first sample.

## Actions

| Action | Preview |
|---|---|
| Open a project or a session | the conversation, revealed in the sidebar |
| Add project | the project editor |
| Update host: Check, Download, Apply, Retry | `Update.Advance` moves between the reference's example states; a stale click does nothing |
| Tunnel Edit, Save | `Tunnels.SavePorts`: tunnel_invalid_ports or tunnel_policy_changed toast, the draft stays |
| Tunnel Open | info toast with the local address (tunnel_preview) |
| Tunnel Close | removed from the list |
| Forward: open address | preview_notice |
| Forward: copy address | the clipboard |
| Forward: close | removed |
| New forward, Create | `AddPort` (port_invalid, port_exists), listed, preview_notice |

Failures show only as toasts, once each (`State.report`): the runtime's
detail, the tunnels' with Retry (which reloads them), the update's and the
metrics' detail.

## Wiring a backend

Fill `Host` from the live host (`live/host.rs` info and snapshot) and the
workspace: `Runtime` from the Node's runtime snapshot, `Tunnels` from the
tunnel policy and list, `Update` from the host updater (replace
`Advance` with the request for `Primary`'s action), `Metrics` from the
monitor's samples, `Ports` from `sailry_client::ports::Forwarder`s.
`Actions` stay; Open address becomes `OpenURL(Port.Address())`.

## UI state vs backend data

| UI state (`State`, `App.Host`, `App.Metrics`) | Backend data (`Host`) |
|---|---|
| The dialogs, their drafts, the picker's query and choice, the failures already toasted, the chosen host, the metrics column | Everything in `Host` |

## Tests

`internal/host/host_test.go`: formats, the tunnel policy's validation and
stale saves, the update's phases and stale clicks, new forwards.
`internal/shell/hosts_test.go`: the host page with its metrics, the
update dialog's Check, toggling the metrics, closing the page, the policy
editor.

## Deviations

| Deviation | Reason |
|---|---|
| The page combines the preview overview (`workspace/host.rs`) with the live header's platform logo and forwards | Phase 6 asks for one host page on preview data; the live monitor (gauges, trends, process actions) needs a Node |
| Port forwards show on the preview page | The reference shows them only on a live host |
| The rail's Add host opens Settings › Connections | The add dialog lives with the Connections page |
| Choosing a host does not switch the sidebar's projects | The preview's sidebar lists every host's projects |
