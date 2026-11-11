# Host

A host's page: the host's monitor in the main column beside the sessions'
side column, a summary of it in the right column, the update dialog and the
port forwards (the reference implementation `live/host.rs`,
`live/host/{monitor,resources,cpu,gauges,trend,network,disks,processes}.rs`,
`workspace/host.rs`, `workspace/host/{activity,runtime,update}.rs`,
`workspace/model/{runtime,updates}.rs` and `ports/{menu,picker,view}.rs`).

## Package and files

`internal/desktop/host`

| File | Holds |
|---|---|
| `model.go` | **The view model**: `Host`, `Project`, `Activity`, `Runtime` (`Facts`, `UptimeParts`, `Protocol`), `Update` (`Phase`, `Primary`, `Advance`, `Headline`), `Metrics`, `Disk`, `Process` (`Bytes`, `Percent`, `Rate`, `Usage`), `Port` (`AddPort`) |
| `sample.go` | What the monitor makes of the samples: the history (`Point`, `Metrics.Follow`, `HistoryLimit`), the real volumes (`RealVolume`, `Metrics.Volumes`, `Metrics.Volume`), `SortProcesses`, `Scaled`, `Clock`, `Load` |
| `page.go` | `State`, `Actions`, `Page`: header (`Host.Connection`), running agents, projects, runtime; `Platform`; failure toasts |
| `monitor.go` | The monitor's cards: processor and cores, memory, utilization, network, the chosen volume's transfers, storage; their place until the first sample |
| `processes.go` | The processes: toolbar, search, the sortable table, its skeleton |
| `charts.go` | The series' tones and the drawing on MyGo's `ui/chart`: area charts, sparklines, rings |
| `update.go` | The update dialog, `OpenUpdate` |
| `metrics.go` | `MetricsPanel`, the right column's summary |
| `menu.go` | What the rail's host menu opens (the port picker, a forward's address, Remove pairing) and the removal's confirmation |
| `ports.go` | The header's forwards, the port picker and the new forward dialog |

Preview data: `preview.HostPages(d)` (`internal/desktop/preview/hosts.go`), one
`Host` per `Data.Hosts` entry: projects, counts and running sessions from
the dataset, this host's runtime, the paired host's unknown runtime and
update, a minute of example samples around the values of `Snapshot::sample`
(cores, load averages, transfers and two processes, on a fixed clock) and two
forwards.

## Entry point

The rail's host menu (`shell.App.hostPicker`): choosing a host sets
`App.Host` and opens `HostPage` (`Shell::select_host`); the host's Update
opens the page with its update dialog; the right-click menu's Host opens
the current host's page, and its Port mappings, Update and Remove pairing
items (`shell.App.hostMenu`, docs/shell.md) open the page with the port
picker (`State.OpenPorts`), the update dialog or the removal's confirmation
(`State.AskRevoke`). Add host opens Settings › Connections with the
Add host dialog (`settings.State.AddHost`). The hosts are listed there, in
the rail's picker, and nowhere else, as in the reference
(`shell/rail/hosts.rs`): the page's side column is the sessions' sidebar
(`App.HasNavigation`, `feature_navigation` in `shell/rail.rs`). The header
shows the host's name and, over the summary column, `metrics_title`; the
details button and ⌥⌘B toggle the summary (`App.Metrics`, the reference's
per-page panel), which starts closed as the reference's
(`panel_open` of `Page::Host`, `preview.rs`); ⌘W in the column closes it, ⌘W
in the page closes the page.

```go
h, st := a.hostPage()                // preview.HostPages, kept per host
host.Page(c, st, h, host.Actions{...}) // main column
host.MetricsPanel(c, st, h)          // right column, when open
```

## Layout

The page is 1040 wide at most (`host.Width`, `WIDTH` in `live/host.rs`),
centered, padded 24, its parts 14 apart, in the reference's order:

| Part | Shows | Built with |
|---|---|---|
| Header | The platform logo in a 44 round tile; the name (18 semibold) beside the status; the platform line once; `host_uptime` (the computer's), `host_node_uptime` (Sailry's) and the processes' count; the first three forwards and Port forwarding; Update host on a host updated from here | MyGo rows and text; CrychicUI `feedback.ConnectionStatus` for a live host's status (`live_connected`, `connection_connecting`, `connections_offline`); `portsHeader`; the preview shows `workspace_host_status` |
| Processor | The load (32 bold, code font), `host_cores`, `host_load` with the three averages and their rings; every core's load after a divider | `kit.Card`; MyGo `ui.Progress` per core; rings on `ui/chart` sectors |
| Memory | Used of the whole with its ring of used and cached; used, cached and swap; the last minute | `kit.Card`; `ui/chart` |
| Utilization | The processor's and the memory's use over the last minute, 0 to 100%, the samples' times under it | `kit.Card`; `ui/chart` `Plot`, `Area`, `Line` |
| Network, Disk | Side by side, each wrapping under the other below 380: download and upload rates over the totals since boot; the chosen volume's write and read rates; both over the last minute | `kit.Card`, `kit.Tag` for the mount; `ui/chart` |
| Storage | `host_partition_count` and one tile per real volume: mount, read and write rates, file system, used of total, a capacity bar. A click chooses the volume the Disk card follows | `kit.Card`; MyGo toggle buttons with `kit.GhostLook`; `ui.Progress` |
| Processes | The count of every process, a search, a note when the sample lists fewer than run, and the table: name, the load with its last minute, the memory with its share, the PID. A header sorts; the load sorts first | `kit.Card`, `kit.Toolbar`, `kit.Result` when empty; MyGo `ui.Table` |
| Running Agents | The running and waiting sessions (agent or waiting user, title, project / branch, model, status); a click opens the session. Absent when there are none | `kit.Group`, `kit.Card`, `kit.Badge` |
| Projects | Rows opening the project's first conversation, Add project in the group's header; the empty state with Add project | `kit.Group`, `kit.List`, `kit.Result`, `kit.Primary` |
| Runtime | The host's type, its counts (projects, worktrees, sessions, terminals, running agents), the version, the time zone and the protocol, `host_unknown` for unknown ones | `kit.Group`, `kit.Card` |

Until the first sample the processor's, the memory's and the processes'
cards hold their place with `kit.Skeleton`. A live host whose sampling
failed, or which is offline, shows `host_sampling_failed` in a `kit.Alert`
in place of the cards and keeps the rest of the page; the failure's detail
is a toast. The cards that put two blocks side by side divide them with a
line only while both fit, and stack them otherwise.

The page has no Remove pairing button ([removed-ui.md](removed-ui.md)): the
rail menu's item opens its confirmation here (`Actions.Revoke`; `menu.go`),
which names the host and the consequence once (`live_revoke_confirm`); the
item is disabled while the removal runs (`Host.Revoking`). The live window
binds it for every paired host (`Runtime.revokeHost`, the `SetTrust`
Settings › Connections uses); a failure toasts, and a host whose pairing is
removed leaves the menu. The preview's tunnels section is not ported: the
live host has none.

Only real volumes show (`host.RealVolume`): the Go sampler lists every
mount, the reference's lists the volumes a file browser shows. Device,
memory and kernel file systems, the system's own mounts (`/System/Volumes`,
`/private/var`, `/dev`, `/proc`, `/sys`, `/run` except `/run/media`) and
volumes of no size are left out of the page, of its summary and of the
partition count.

The summary column (its title is in the header band, docs/shell.md, without
the chart icon): the preview note, the host, the processor and the memory
each with its bar and its last minute, the network rates, the real volumes
with how full each is, and the five busiest processes; an indeterminate bar
and the processes' skeleton until the first sample, which the preview
delivers after `preview.LoadAfter`.

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
`InspectHost` supplies its hostname, system, architecture and both uptimes:
the computer's (`host_uptime_secs`) and the Node's own (`node_uptime_secs`),
which the header shows apart, since a Node started minutes ago runs on a
computer up for days. The system and architecture are the header's platform
line and no runtime fact. `Host.Offline` is whether the Node is not
connected. The Link
handshake supplies a remote Node's advertised software and protocol versions;
the local Node uses the running build's versions. A controller and a Host are
compatible when their protocol versions are equal (`internal/link/compatibility.go`);
the software version is shown and offered for update, never compared for
compatibility. The frozen HostInfo contract
has no remote timezone, so that fact stays unknown. Runtime read failures toast
once without replacing the page. Remembered offline execution peers remain in
the host list; nonexecution controllers do not receive Node reads.

Projects, worktrees, sessions and open terminals come from that Node's own Client
snapshot. Running and waiting rows use Client's aggregate activity lanes,
found once per `Client.Revision`, excluding delegated and archived sessions. Their navigation retains the original
Node identity. A shared Network owns one remote Client per execution peer, while
each window retains its own presentation and selection.

The host page requests detailed samples while it shows its host, whether or
not the summary column is open (`detailed` in `bind_host_monitor`). The
optional sidebar footer requests summary samples from its selected execution
Node. Both use `ReadHostMetrics`, targeting that Node explicitly, at the
source's one-second cadence. Hiding both consumers stops sampling and cancels
their outstanding read; there is no invented metrics subscription topic.
`hostMetrics` maps every value of the sample: the load, the cores, the load
averages, memory (used is what is not available, cached what is available
and not free), swap, the network's rates and totals, every disk with its
transfer rates, and the processes with their PID and start. Missing values
remain unknown and show as a dash or a gap in a chart.

The history is the desktop's: each sample follows the one before
(`Metrics.Follow`, `View::accept` in `crates/client/src/host.rs`), a minute
at most, started again after a pause of more than five seconds, and each
process's load by its PID and start. Metrics failures toast once, and
retained samples are kept while a healthy new read is pending. The last
sample remains private for ordering and for the history, but failed sampling
or a disconnected owner hides its values in the page, the summary and the
sidebar.

The execution Node routes `stop_host_process` for its own machine, although
the page has no control that sends it. It refuses plugin callers, PID 0,
PID 1 and the Node's own process, signals only a process whose start time still
equals the listed one, and offers only the forced stop on Windows.

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

An update whose result was not observed keeps its request and offers Check
status, which asks the Node what became of that request
([node.md](node.md#unobserved-outcomes)). A recorded success shows Current and
a recorded failure is a plain failure. A result the Node recorded as
unconfirmed, or lost to a restart, is final: asking again cannot change it, so
the uncertain toast is shown once, the request is dropped and Check status
reads the Host's version instead. The Host then shows Current, or the update
is offered again under a new request. Before it confirms an update, the Node
drops its cached connection to that Host (`Link.Disconnect`) and drops it
again after every probe that did not confirm, so a connection to the replaced
process is never the one that answers.

Add host uses the local Node's `SaveSsh`, `CheckSsh`, explicit `TrustSsh` and
`InstallHost` sequence. Passwords, inline private keys, key paths and passphrases
are passed only to Node-owned configuration. A host-key prompt shows the exact
endpoint, algorithm and fingerprint, including changed-key status; opening or
dismissing it never grants trust. Key-file selection uses the native window
picker. Install progress retains the original request identifier.

The dialog also offers the SSH agent, which has no secret. The Node reaches
the agent through `SSH_AUTH_SOCK`, a Unix socket, so the method is not offered
on Windows. One dialog saves one connection: a changed draft updates it at its
revision with its trusted host key, and a secret left blank under the same
authentication sends no credential, which keeps the saved one. A save refused
with a conflict forgets the saved connection, so the next one saves anew; any
other refusal, as of a shared project that went away, keeps it. The Host update
form saves the same way. SSH methods choose the connection's sharing: private,
global, or the local Node's projects checked in the list; project sharing needs
at least one project.

On Windows the dialog offers WSL instead of the agent. It lists the local
Node's WSL2 distributions (`ListWslDistributions`) and installs with
`InstallWslHost` in one step, with `ReadWslHostInstall` progress and
`CancelWslHostInstall`; its result ends the dialog as an SSH installation's
does. WSL behavior is verified only on Windows.

An uncertain completion keeps that request, locks the draft and turns the
primary button into Check status, which checks `Client.Outcome` and never
sends the installation again. A recorded success completes the dialog and a
recorded failure is a plain failure that unlocks the draft. Admitted work is
never replayed; an explicitly not-admitted request is sent with its original
identifier. A receipt the Node no longer has cannot be confirmed: the
uncertain toast tells the user to check the host, and the dialog takes a new
draft. A failed connection test (unreachable host, refused login, missing
agent) changes nothing and is a plain failure. Closing the dialog abandons an
installation left to check. Network-unconfirmed installation recovery checks
the old address, then the selected network hints, verifies the returned Node
identity and effective selection, and remembers only the confirmed address.
While a step runs the primary button shows a spinner and is disabled; a step
cancelled here that still runs on the Node reports busy instead of starting
another. A pairing being removed shows a spinner in place of its menu.

The sharing side serves pairing connections only while an invitation is open
and gives one invitation at most four concurrent claimants; any other pairing
connection is closed at once, so the joiner sees a network fault instead of the
`permission_denied` result frame. A wrong or stale code against an open
invitation still receives that frame. A claimant whose trust could not be
stored releases the invitation for another device. Pairing is bidirectional as
in Rust: either paired side may command the other.

Removing a pairing is one-sided. It deletes this device's record of the peer
and closes the connections between them; the peer keeps its record of this
device and is refused when it connects. It does not stop work the Host is
already running: sessions and queued turns, commands, terminals and anything
else this controller started there continue, and stay reachable from the
Host's other controllers. Stop that work before removing the pairing.

A paired controller may send every command except the few the Node keeps for
the controller on its own device:

| Command | Refusal for a paired controller | Check |
|---|---|---|
| `create_session_at` for another Node (session forwarding) | `permission_denied`, "only the local controller can forward configuration to another Node" | `store.PrepareSessionForward` |
| `import_session` | accepted only from the paired Node named as its source, never from the Node's own controller | `store.importSession` |
| A dispatch job's callback | a controller that repeats the job's request identifier gets none of the job's package authority | `store.DispatchPackage` |
| A plugin request scoped to a turn or to a running invocation | `permission_denied`, "plugin turn belongs to the execution Node" or "plugin invocation does not own this action" | `store.PluginRequestPackage`, `validatePluginRequestDB`, `pluginCallback`, `plugin_connection.go` |
| `request_computer_permission` | `permission_denied`, "system permissions must be granted on the execution device"; `read_computer_permissions` answers with `local: false` | `computer.Probe.Request` |
| `count_feature` | `permission_denied`, "feature counters require the local controller" | `dispatchFeature` |

A plugin scope reaches no network command (`read_network_settings`,
`save_network_settings`, `list_network_presets`, `probe_network`), port
command, WSL command, backup command, `inspect_host`, `read_host_metrics` or
`stop_host_process`. A package that declares `manage_ssh` may save, trust and
remove an SSH connection within its scope and install or update a Host
through it, as in Rust (`store/plugins/connections.rs`); a Host installed
that way is paired like any other.

Only the sharing side decides whether a ticket is still valid: it holds the
60-second deadline and its token. The joining side does not compare the
ticket's expiry with its own clock, and neither does it for a ticket the
pairing service released for a PIN, because the two machines' clocks differ.
An installation pairs with the Host it installed through the ticket the Host
service publishes, and the Host replaces a ticket that expired. The service
has only just started, so a refused or unreachable attempt reads the ticket
again and pairs again, at most five times within the 90 seconds the
installation waits; SSH and WSL installations do the same. A result that may
have changed trust is not retried.

Port actions are additive callbacks over the accepted picker. Each window owns
its Client Forwarders, binds only a loopback listener, and releases its forwards
on window close. Open uses the confirmed local address through the native
opener; Create reports backend failure while retaining the entered ports. A
port stream that fails to open is released on the Node with `cancel_port` even
when the caller's cancellation caused the failure; that cleanup waits at most
two seconds.

A forward fails when its Host's connection is lost, and its row shows it as
not listening. When the Host is connected again the window creates the forward
again on the same local port, from the same announced service when it had
one, at most three times for each return of the Host; a forward that cannot
be created (the service ended, the local port was taken) stays not listening.
A forward the user or its service closed is not created again.

A session on a remote Host can use a provider configured only on this Desktop:
the local Node forwards the provider, a transferable secret and the roles over
the paired Link, and the Host keeps them private to that session
([node.md](node.md#session-forwarding)). The secret travels Node to Node and
never reaches a controller. The Desktop composer sends it
([composer.md](composer.md#session-creation-and-forwarding)).

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
| The dialogs, their drafts, the picker's query and choice, the failures already toasted, the chosen host, the summary column, the chosen volume, the processes' search and sort | Everything in `Host`, the samples' history included |

## Tests

`internal/desktop/host/host_test.go`: formats, the runtime's facts, the
header's status, the history (`TestFollow`), the real volumes, the processes'
sort and search, the update's phases and stale clicks, new forwards, and
`TestPage`: a live host's sections in order, the header once, real volumes
only in the page and its summary, the offline page.
`internal/desktop/shell/hosts_test.go`: the host page, the update dialog's
Check, the summary column closed until toggled, closing the page;
`testHostLayout`: the side column beside the page, the sections' order, the
header once, the summary's volumes, the hosts in the rail's picker; the rail
menu's forwards, Update and Remove pairing with its confirmation, cancel and
busy state. `internal/desktop/shell/hosts_snapshot_test.go` writes the review
sheets when `SAILRY_SNAPSHOTS` is set. `internal/desktop/live/hosts_metrics_test.go`:
a sample's mapping.

## Deviations

| Deviation | Reason |
|---|---|
| One page serves the preview and the live host: the live monitor (`live/host.rs`) followed by the overview's projects and runtime (`workspace/host.rs`) | The reference has two pages; the live one has no projects list or runtime facts, which the accepted page shows and its Add project needs |
| Port forwards and the monitor show on the preview page, on example samples | The reference shows them only on a live host |
| No GPU card; the memory card takes the row | The Go Node's sampler reads no GPU, so `HostMetricsSample.gpus` is always empty |
| A process has no right-click menu (Details, Copy PID, End process, Force quit) | The Node routes `stop_host_process`, but no desktop action sends it: wiring it is backend work outside this page |
| Cores are MyGo progress bars with their percentage, and a volume's capacity is a horizontal bar | The reference draws lit cells per core and a vertical capacity bar by hand; MyGo's `ui.Progress` shows the same values |
| Charts and rings are drawn on MyGo's `ui/chart` in `charts.go` | MyGo has the scales, areas, lines and sectors but no finished card chart; CrychicUI's `chart` package is not used ([crychic.md](crychic.md#charts)) |
| The right column stays, as a summary, closed until toggled | The reference's column is the preview's metrics panel, also closed by default; the accepted toggle and ⌥⌘B keep working |
| The desktop filters the sampler's mounts to real volumes | The Go sampler lists device and system mounts that the reference's never did; the Node is not changed here |
| The rail's host rows have no Update button; the picker's right-click menu offers Update for each host that has one | The approved picker rows are a name and a check; the reference's trailing Update would change them |
| The rail's Add host opens Settings › Connections | The add dialog lives with the Connections page |
| Choosing a host does not switch the sidebar's projects | The preview's sidebar lists every host's projects |
| Update Download prepares the update instead of acquiring a release | The frozen protocol has a single install/update command. Apply runs it: the remote machine downloads and verifies the release feed's installer and archive, or the execution Node uploads `host-artifacts/<goos>-<goarch>/` when that directory holds a locally built `sailry-host` |
| A port forward keeps its `node` subscription only as a liveness signal | The Rust forwarder also projected that subscription and recovered it after a reset, without reading the projection; the Client owns the Node projection, and a forward ends when its subscription ends. Rust leaves a failed forward to the user; the window creates it again when the Host returns |
| The joining side does not check a pairing ticket's expiry | Rust rejects a ticket that its own clock finds expired or more than 60 seconds from expiry (`link/src/network/pairing.rs`, `rendezvous.rs`), so a correct ticket from a Host whose clock was 7.4 seconds ahead was refused. The sharing side enforces the lifetime and the token |
| Installation pairing is tried up to five times | Rust pairs once (`ssh/install.rs`, `wsl/install.rs`) and one early refusal left an installed Host unpaired |
| An update's unconfirmed result is released by Check status | Rust keeps offering the same request; a final unknown result cannot change, so the Host's version is read instead |
| Update confirmation drops the cached connection after each unconfirmed probe as well as before the first | Rust disconnects once before probing; a probe that reached the old process would otherwise leave its connection for the next |
| Remote timezone remains unknown | HostInfo and the Link handshake do not expose it; the controller does not substitute its own timezone |
| The sidebar footer keeps its summary samples | It shows the selected Node's load without the page's detail |
