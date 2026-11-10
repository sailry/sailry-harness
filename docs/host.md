# Host

A host's page: its overview in the main column and its metrics in the
right column, with the update dialog and the port forwards
(the reference implementation `workspace/host.rs`, `workspace/host/{
activity,metrics,metrics/data,runtime,update}.rs`,
`workspace/model/{runtime,updates}.rs`, the header of
`live/host.rs` and `ports/{menu,picker,view}.rs`).

## Package and files

`internal/desktop/host`

| File | Holds |
|---|---|
| `model.go` | **The view model**: `Host`, `Project`, `Activity`, `Runtime` (`Facts`, `UptimeParts`, `Protocol`), `Update` (`Phase`, `Primary`, `Advance`, `Headline`), `Metrics` (`Bytes`, `Percent`, `Rate`, `Usage`), `Port` (`AddPort`) |
| `page.go` | `State`, `Actions`, `Page`: header, facts, projects, runtime, activity; `Platform`; failure toasts |
| `update.go` | The update dialog, `OpenUpdate` |
| `metrics.go` | `MetricsPanel`, the right column |
| `ports.go` | The header's forwards, the port picker and the new forward dialog |

Preview data: `preview.HostPages(d)` (`internal/desktop/preview/hosts.go`), one
`Host` per `Data.Hosts` entry: projects, counts and running sessions from
the dataset, this host's runtime, the paired host's unknown runtime and
update, the metrics sample of
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
user, title, project / branch, model, status; a click opens the session).
The preview's tunnels section is not ported: the live host has none.

The metrics column: a 40 DIP header with the chart icon, the preview note,
the host, CPU and memory with its bar, network rates, disks and the
busiest processes, an empty card where there are none, an indeterminate
bar and the processes' skeleton (five table rows, `kit.Skeleton`) until
the first sample, which the preview delivers after `preview.LoadAfter`.

## Actions

| Action | Preview |
|---|---|
| Open a project or a session | the conversation, revealed in the sidebar |
| Add project | the project editor |
| Update host: Check, Download, Apply, Retry | `Update.Advance` moves between the reference's example states; a stale click does nothing |
| Forward: open address | preview_notice |
| Forward: copy address | the clipboard |
| Forward: close | removed |
| New forward, Create | `AddPort` (port_invalid, port_exists), listed, preview_notice |

Failures show only as toasts, once each (`State.report`): the runtime's
detail, the update's and the metrics' detail.

## B7 live wiring

`internal/desktop/live/hosts*.go` owns each execution Node's presentation.
`InspectHost` supplies its hostname, system, architecture and uptime. The Link
handshake supplies a remote Node's advertised software and protocol versions;
the local Node uses the running build's versions. The frozen HostInfo contract
has no remote timezone, so that fact stays unknown. Runtime read failures toast
once without replacing the page. Remembered offline execution peers remain in
the host list; nonexecution controllers do not receive Node reads.

Projects, worktrees, sessions and open terminals come from that Node's own Client
snapshot. Running and waiting rows use Client's aggregate activity lanes,
excluding delegated and archived sessions. Their navigation retains the original
Node identity. A shared Network owns one remote Client per execution peer, while
each window retains its own presentation and selection.

The visible Host metrics panel requests detailed samples. The optional sidebar
footer requests summary samples from its selected execution Node. Both use
`ReadHostMetrics`, targeting that Node explicitly, at the source's one-second
cadence. Hiding both consumers stops sampling and cancels their outstanding
read; there is no invented metrics subscription topic. Missing CPU and memory
remain unknown. Metrics failures toast once, and retained samples are kept while
a healthy new read is pending. The last sample remains private for ordering,
but failed sampling or a disconnected owner hides its values in both the panel
and sidebar. Detailed metrics map the Node's disks and busy processes
into the accepted column without introducing additional monitor controls.

`Actions.Update` captures the displayed Update and rejects stale clicks.
Check reads the remote handshake and compares its version with the local Host
build using semantic-version precedence, ignoring build metadata. Download
prepares the local-artifact update path; it performs no release download and
does not invent transfer progress. Apply opens the source's SSH credentials
form and warns that the target Host restarts. The frozen protocol exposes one
`UpdateSshHost` operation, so Node owns artifact verification, upload, activation
and reconnect confirmation. `ReadHostInstall` supplies connecting, detecting,
installing and pairing stages. Only a confirmed matching Node result displays
Current. Release catalog acquisition belongs to B14.

Add host uses the local Node's `SaveSsh`, `CheckSsh`, explicit `TrustSsh` and
`InstallHost` sequence. Passwords, inline private keys, key paths and passphrases
are passed only to Node-owned configuration. A host-key prompt shows the exact
endpoint, algorithm and fingerprint, including changed-key status; opening or
dismissing it never grants trust. Key-file selection uses the native window
picker. Install progress retains the original request identifier.

An uncertain completion keeps that request and checks `Client.Outcome` on
Retry. Admitted work is never replayed; an explicitly not-admitted request can
be sent with its original identifier. A terminal unknown result requires target
inspection and does not start another installation. Network-unconfirmed
installation recovery checks the old address, then the selected network hints,
verifies the returned Node identity and effective selection, and remembers only
the confirmed address.

Port actions are additive callbacks over the accepted picker. Each window owns
its Client Forwarders, binds only a loopback listener, and releases its forwards
on window close. Open uses the confirmed local address through the native
opener; Create reports backend failure while retaining the entered ports.

## Host task automation

`cmd/sailry-host` and `internal/hostctl` serve noninteractive `submit` and `wait`
through the running Host's profile-private, owner-only control socket. The
management process never opens another Node or business database. Detailed
options and exit codes are in [Contributing](../CONTRIBUTING.md#host-automation).

Submitting to a registered worktree reads its captured project catalog, then
uses `start_session`. The existing command transaction creates and names the
session, stores its optional immutable association and admits its first turn
atomically. Failed admission rolls back the new session and turn together.
Explicit or default configuration, model/team selection, team revision,
instructions and providers are frozen through the ordinary turn capture.
An association identifies an external object and grants no resource authority.

Submitting to an existing session uses `submit_task`, with an optional expected
session revision. It enters the ordinary ready task queue, including when
another turn is running; it does not become guidance. Both commands retain the
original request identifier and use the same Agent loop, canonical history,
approvals and questions as controller submissions. New task input supersedes a
completed planning review through the existing canonical review path.

`wait` observes the specified session and turn without resubmitting input or
answering approvals. Closing that observation or disconnecting a controller
leaves admitted work running. Uncertain submission recovers only with the
original request and its recorded outcome.

## UI state vs backend data

| UI state (`State`, `App.Host`, `App.Metrics`) | Backend data (`Host`) |
|---|---|
| The dialogs, their drafts, the picker's query and choice, the failures already toasted, the chosen host, the metrics column | Everything in `Host` |

## Tests

`internal/desktop/host/host_test.go`: formats, the update's phases and stale
clicks, new forwards. `internal/desktop/shell/hosts_test.go`: the host page with
its metrics, the update dialog's Check, toggling the metrics, closing the
page.

## Deviations

| Deviation | Reason |
|---|---|
| The page combines the preview overview (`workspace/host.rs`) with the live header's platform logo and forwards | Phase 6 asks for one host page on preview data; the live monitor (gauges, trends, process actions) needs a Node |
| Port forwards show on the preview page | The reference shows them only on a live host |
| The rail's Add host opens Settings › Connections | The add dialog lives with the Connections page |
| Choosing a host does not switch the sidebar's projects | The preview's sidebar lists every host's projects |
| B7 update Download prepares a local-artifact update instead of acquiring a release | B7 installs locally built Go Host artifacts; the frozen protocol has a single install/update command and B14 owns release acquisition |
| Remote timezone remains unknown | HostInfo and the Link handshake do not expose it; the controller does not substitute its own timezone |
| Live metrics keep the accepted column and sidebar footer | B7 supplies real samples without restoring the Rust monitor's additional gauges, trends or process actions |
