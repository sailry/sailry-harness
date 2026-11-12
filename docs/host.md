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
| `model.go` | **The view model**: `Host`, `Activity`, `Runtime` (the version, the protocol and the uptimes; `UptimeParts`, `Protocol`), `Update` (`Phase`, `Primary`, `Advance`, `Headline`), `Metrics`, `Disk`, `GPU`, `Process` (`Bytes`, `Percent`, `Rate`, `Usage`), `Port` (`AddPort`) |
| `sample.go` | What the monitor makes of the samples: the history (`Point`, `Metrics.Follow`, `HistoryLimit`), the chosen volume and accelerator (`Metrics.Volume`, `Metrics.GPU`), `SortProcesses`, `Scaled`, `Clock`, `Load` |
| `page.go` | `State`, `Actions`, `Page`: header (`Host.Connection`, the platform and versions line), running agents; `Platform`; failure toasts |
| `monitor.go` | The monitor's cards: processor and cores, the accelerator, memory, utilization, network, the chosen volume's transfers, storage; their place until the first sample |
| `processes.go` | The processes: toolbar, search, the sortable table, its skeleton |
| `process_actions.go` | A process's right-click menu (`State.processMenu`), its details and the stop confirmation (`processDialogs`), `Process.Stoppable` |
| `charts.go` | The series' tones and the drawing on MyGo's `ui/chart`: area charts with the tip of the sample under the pointer (`hover`, `readout`), sparklines, rings |
| `update.go` | The update dialog, `OpenUpdate` |
| `metrics.go` | `MetricsPanel`, the right column's summary |
| `menu.go` | What the rail's host menu opens (the port picker, a forward's address, Remove pairing) and the removal's confirmation |
| `ports.go` | The header's forwards, the port picker and the new forward dialog |

Preview data: `preview.HostPages(d)` (`internal/desktop/preview/hosts.go`), one
`Host` per `Data.Hosts` entry: running sessions from
the dataset, this host's runtime (its version, protocol and uptimes), the
paired host's unknown runtime and update, a minute of example samples around the values of `Snapshot::sample`
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
| Header | The platform logo in a 44 round tile; the name (18 semibold) beside the status; under it one muted line, cut short with an ellipsis: the platform, then the host's version and the protocol's after their labels (`host_version`, `host_protocol`), as `darwin · 27.2 · arm64 · Host version 0.1.6 · Protocol version v1`, a version not known left out (`Host.detail`); `host_uptime` (the computer's), `host_node_uptime` (Sailry's) and the processes' count; the first three forwards and Port forwarding; Update host on a host updated from here | MyGo rows and text; CrychicUI `feedback.ConnectionStatus` for a live host's status (`live_connected`, `connection_connecting`, `connections_offline`); `portsHeader`; the preview shows `workspace_host_status` |
| Processor | The load (32 bold, code font), `host_cores`, `host_load` with the three averages and their rings; every core's load after a divider | `kit.Card`; MyGo `ui.Progress` per core; rings on `ui/chart` sectors |
| GPU | Only when the sample has an accelerator: its load (32 bold), its name, the load's bar and the last minute; a `kit.Select` chooses among several | `kit.Card`, `kit.Select`; MyGo `ui.Progress`; `ui/chart` |
| Memory | Used of the whole with its ring of used and cached; used, cached and swap; the last minute | `kit.Card`; `ui/chart` |
| Utilization | The processor's, the chosen accelerator's and the memory's use over the last minute, 0 to 100%, the samples' times under it | `kit.Card`; `ui/chart` `Plot`, `Area`, `Line` |
| Network, Disk | Side by side, each wrapping under the other below 380: download and upload rates over the totals since boot; the chosen volume's write and read rates; both over the last minute | `kit.Card`, `kit.Tag` for the mount; `ui/chart` |
| Storage | `host_partition_count` and one tile per real volume: mount, read and write rates, file system, used of total, a capacity bar. A click chooses the volume the Disk card follows | `kit.Card`; MyGo toggle buttons with `kit.GhostLook`; `ui.Progress` |
| Processes | The count of every process, a search, a note when the sample lists fewer than run, and the table: name, the load with its last minute, the memory with its share, the PID. A header sorts; the load sorts first. The selected row is the neutral fill. A right-click opens the OS-native menu: Details, Copy PID, Copy name, End process, Force quit | `kit.Card`, `kit.Toolbar`, `kit.Result` when empty; MyGo `ui.Table` in `theme.Neutral`, `ListState.CellMenu`; `kit.AlertDialog`, `kit.Dialog` |
| Running Agents | The running and waiting sessions (agent or waiting user, title, project / branch, model, status); a click opens the session. Absent when there are none | `kit.Group`, `kit.Card`, `kit.Badge` |

The page ends with the running agents, or with the processes on a host
that has none. It has no projects section ([removed-ui.md](removed-ui.md)):
the host's information does not list its projects or add one; projects are
added from the sidebar, the rail's host menu and the composer's project
menu, and opened from the sidebar. It has no runtime section either
([removed-ui.md](removed-ui.md)): of that grid's facts the header's line
keeps the host's version and the protocol's, and the rest (the host's type,
its counts of projects, worktrees, sessions, terminals and running agents,
the time zone) show nowhere.

Every area chart reads the sample under the pointer (`hover` in
`charts.go`): the GPU's and the memory's last minute, the utilization, the
network and the disk, and the summary column's three. A 1 px guide stands at
the sample nearest the pointer with a dot on each series, and beside it a
tip names the sample's time (as the axis writes it, `Clock`) over one row a
series: its colour mark, its name and its value as the card writes it
(`12.5%`, `512.0 KiB/s`, and the memory card's use as a size, `8.0 GiB`), a
dash for a value the sample did not have. The tip is Kit's floating surface
(`kit.Floating`) in the window's overlay, attached to the guide as Kit's
tooltip is to its anchor (`kit.Attach`, to the right, on the left where the
window ends), names at 12 and values at 14 in the code font in a column of
fixed width. The sample is found again from the pointer in every frame, so
under a resting pointer the guide and the tip stay where they are while the
values change in place as samples arrive. It goes when the pointer leaves
the plot or a menu or popover opens (`kit.TipsHidden`); the tip and the
marks take no input, so the wheel scrolls the page through them. The rings
and the processes' sparklines have no tip, as in the reference.

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

Only real volumes show, and the Node decides which: its sampler lists the
mounts `files.IsVolume` keeps (`internal/node/files/browser_volumes.go`),
the rule the file picker's volume list uses. Device, memory and kernel file
systems, macOS mounts marked `nobrowse`, the system's own mounts
(`/System/Volumes`, `/private/var`, `/dev`, `/proc`, `/sys`, `/run` except
`/run/media`) and volumes of no size are never sampled, so the page, its
summary and the partition count show every disk of a sample.

A process's menu follows the process, not its row (`ListState.Key` is the
PID with its start). End process and Force quit are disabled for a process
the Node always refuses (PID 0 and 1, one without a start), while another
stop request is on its way, and where no action is bound; a Windows host
has no End process, because its Node offers only the forced stop. Either
asks once, naming the process, its PID and the consequence
(`host_process_stop_confirm`, `host_process_force_confirm`).

In Demo mode the hosts stay the window's own (docs/shell.md), so a live
window shows its live hosts. Over the preview's hosts the page speaks as a
live host's does (`Host.plain`): the status is the connection's, the
loading, update and failure texts are the live ones, the preview's notes
(`workspace_host_status`, `metrics_preview`, `host_activity_hint`,
`update_preview`) are left out, the example
processes show as programs (`demoProcesses`) and an action raises no
`preview_notice` (`kit.PreviewNotice`). What the actions do is still the
preview's. The preview's example values (its version, platform line and
mount) are data and stay.

The summary column (its title is in the header band, docs/shell.md, without
the chart icon): the preview note, the host, the processor and the memory
each with its bar and its last minute, the network rates, the real volumes
with how full each is, and the five busiest processes; an indeterminate bar
and the processes' skeleton until the first sample, which the preview
delivers after `preview.LoadAfter`.

## Actions

| Action | Preview |
|---|---|
| Open a session | the conversation, revealed in the sidebar |
| Update host: Check, Download, Apply, Retry | `Update.Advance` moves between the reference's example states; a stale click does nothing |
| Forward: open address | preview_notice |
| Forward: copy address | the clipboard |
| Forward: close | removed |
| New forward, Create | `AddPort` (port_invalid, port_exists), listed, preview_notice |
| Process: Details, Copy PID, Copy name | the dialog, the clipboard |
| Process: End process, Force quit | disabled: `Actions.StopProcess` is unbound |

Failures show only as toasts, once each (`State.report`): the runtime's
detail, the update's and the metrics' detail.

## B7 live wiring

`internal/desktop/live/hosts*.go` owns each execution Node's presentation.
`InspectHost` supplies its hostname, system, architecture and both uptimes:
the computer's (`host_uptime_secs`) and the Node's own (`node_uptime_secs`),
which the header shows apart, since a Node started minutes ago runs on a
computer up for days. The system and architecture are the header's platform
line. `Host.Offline` is whether the Node is not
connected. The Link
handshake supplies a remote Node's advertised software and protocol versions;
the local Node uses the running build's versions. Both end the header's line
(`Runtime.Version`, `Runtime.ProtocolMin` and `ProtocolMax`). A controller and a Host are
compatible when their protocol versions are equal (`internal/link/compatibility.go`);
the software version is shown and offered for update, never compared for
compatibility. A mismatch toasts the reference's copy chosen from the fault's
details (`host_version_older`, `host_version_newer`, `host_version_unknown`,
`controller_version_unknown`; `feedback.rs` `fault_message`) rather than the
fault's English message, and a Link failure of any kind leaves the page once
the Link reaches the host again.
The Node still reports the computer's time zone (`HostInfo.timezone`, an
optional member with the zone's abbreviation and offset, `CST +08:00`, or
`UTC+08:00` for a zone without a name), but the page no longer shows it and
the desktop does not project it. Runtime read failures toast
once without replacing the page. Remembered offline execution peers remain in
the host list; nonexecution controllers do not receive Node reads.

The page no longer lists that Node's projects, and its worktrees, sessions
and terminals are no longer counted for it. Running and waiting rows use Client's aggregate activity lanes,
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
transfer rates, the accelerators with their load, and the processes with their PID and start. Missing values
remain unknown and show as a dash or a gap in a chart.

The history is the desktop's: each sample follows the one before
(`Metrics.Follow`, `View::accept` in `crates/client/src/host.rs`), a minute
at most, started again after a pause of more than five seconds, and each
process's load by its PID and start. Metrics failures toast once, and
retained samples are kept while a healthy new read is pending. The last
sample remains private for ordering and for the history, but failed sampling
or a disconnected owner hides its values in the page, the summary and the
sidebar.

`Actions.StopProcess` (`Runtime.stopHostProcess`) sends `stop_host_process`
to the Node of the host the page shows, with the listed PID and start and
whether to force. The execution Node routes it for its own machine: it
refuses plugin callers, PID 0, PID 1 and the Node's own process, signals only
a process whose start time still equals the listed one, and offers only the
forced stop on Windows. One request runs for a host at a time
(`Host.Stopping`), and its outcome is one toast (`processStopOutcome`):
`host_process_stop_sent`; a failure for a process that exited
(`host_process_gone`), is protected (`host_process_protected`) or could not
be signalled (`host_process_stop_failed`); a conflict when the PID now
belongs to another process (`host_process_changed`); uncertain when the
result did not arrive (`host_process_uncertain`). A request whose result is
unknown is not sent again.

The Node's sampler reads the accelerators on macOS only: each
`IOAccelerator` service's `Device Utilization %` and model, through IOKit
by purego (`internal/node/host/gpu_darwin.go`), as the reference's
`gpu.rs` does there. gopsutil has no GPU reading, and the pure-Go ways on
Linux and Windows are per vendor or need a helper process, so those Nodes
report none and their hosts have no GPU card.

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
opener; Create reports a forward that could not start as `port_failed`, as the
reference does, while retaining the entered ports. A
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
| The dialogs, their drafts, the picker's query and choice, the failures already toasted, the chosen host, the summary column, the chosen volume and accelerator, the processes' search and sort, the process whose details or confirmation is open | Everything in `Host`, the samples' history included |

## Tests

`internal/desktop/host/host_test.go`: formats, the charts' tip
(`TestChartTip`: the sample under the pointer, another sample, a new value
in place, leaving), the header's status, the history (`TestFollow`), the chosen volume and accelerator
(`TestChoices`), the processes'
sort and search, the update's phases and stale clicks, new forwards, and
`TestPage`: a live host's sections in order, the header once with its
versions, no runtime section, the utilization's tip, every volume
in the page and its summary, the accelerator's card and choice, the
process menu with its confirmation and details, the offline page.
`internal/desktop/shell/hosts_test.go`: the host page, the update dialog's
Check, the summary column closed until toggled, closing the page;
`testHostLayout`: the side column beside the page, the sections' order, the
header once, no runtime section, the summary's volumes, the hosts in the rail's picker; the rail
menu's forwards, Update and Remove pairing with its confirmation, cancel and
busy state; `testHostDemo`: no preview note on the page, its summary, its
update dialog or the rail's picker in Demo mode, the header's versions, no
runtime section and the utilization's tip.
`internal/desktop/shell/hosts_snapshot_test.go` writes the review sheets,
the Demo mode page among them, when `SAILRY_SNAPSHOTS` is set. `internal/desktop/live/hosts_metrics_test.go`:
a sample's mapping, a stop request's outcomes; `ssh_hosts_test.go`: a stop
request sent once to the host's Node, the live header's platform and versions
with no runtime section.
`internal/node/host/metrics_test.go`: the sampler's volumes, accelerators
and time zone; `internal/node/files/browser_test.go`: the volume rule.

## Deviations

| Deviation | Reason |
|---|---|
| Upload is drawn in `chart.4` (violet) beside the download's `chart.2` (blue) | The reference draws upload in `info` and download in `chart_2`, the same blue, so the two series of one chart could not be told apart |
| One page serves the preview and the live host: the live monitor (`live/host.rs`), without the overview's projects (`workspace/host.rs`) | The reference has two pages; its overview lists the host's projects with Add project. User decision, 2026-11-12: the host's information does not show them ([removed-ui.md](removed-ui.md)), as the reference's live page does not |
| No runtime section; the host's version and the protocol's end the header's platform line, after their labels | The reference's overview shows a grid of runtime facts (`workspace/host/runtime.rs`). User decision, 2026-11-12: the grid is removed and the two versions are enough ([removed-ui.md](removed-ui.md)). The labels are the grid's own copy, since two bare versions side by side read as one |
| The charts' tip is built in `charts.go`: the pointer's sample from `PointerPosition` and `chart.Nearest`, MyGo's `chart.RuleX` and `chart.Point` marks, and a `kit.Floating` panel attached to the guide in `ui.Overlay` | The reference's `AreaChart` has a tooltip of its own (`tooltip_title`, `tooltip_value`). MyGo's `ui/chart` is headless and has none; its `chart.Select` keeps the chosen index while the pointer rests, so the guide would drift while the history fills, and makes each chart a stop of Tab. `kit.Tooltip` shows one text after a delay at its anchor's middle. The tip reads every series of the sample at once, each with its mark |
| The memory card's tip reads the use as a size | The history holds the use in percent; the card's figure and legend write sizes, so its tip does too. The utilization's tip reads the same samples in percent, as its legend does |
| Port forwards and the monitor show on the preview page, on example samples | The reference shows them only on a live host |
| A host without an accelerator in its sample has no GPU card, and the card takes a row of its own | The reference shows the card beside the memory with "unavailable"; the Node reads accelerators only on macOS, so a Linux or Windows host would always show an empty card, and the accepted memory card keeps its row |
| A Windows host's process menu has no End process | The reference offers it and the Node refuses it there; only the forced stop exists on Windows |
| Cores are MyGo progress bars with their percentage, and a volume's capacity is a horizontal bar | The reference draws lit cells per core and a vertical capacity bar by hand; MyGo's `ui.Progress` shows the same values |
| Charts and rings are drawn on MyGo's `ui/chart` in `charts.go` | MyGo has the scales, areas, lines and sectors but no finished card chart; CrychicUI's `chart` package is not used ([crychic.md](crychic.md#charts)) |
| The right column stays, as a summary, closed until toggled | The reference's column is the preview's metrics panel, also closed by default; the accepted toggle and ⌥⌘B keep working |
| The Node's sampler filters its mounts by the file picker's volume rule | The reference's sampler lists what `sysinfo` calls disks; gopsutil lists every mount, so the Node leaves the device and system ones out. A Host built before this still lists them, and the desktop no longer hides them |
| The rail's host rows have no Update button; the picker's right-click menu offers Update for each host that has one | The approved picker rows are a name and a check; the reference's trailing Update would change them |
| The rail's Add host opens Settings › Connections | The add dialog lives with the Connections page |
| Choosing a host does not switch the sidebar's projects | The preview's sidebar lists every host's projects |
| Update Download prepares the update instead of acquiring a release | The frozen protocol has a single install/update command. Apply runs it: the remote machine downloads and verifies the release feed's installer and archive, or the execution Node uploads `host-artifacts/<goos>-<goarch>/` when that directory holds a locally built `sailry-host` |
| A port forward keeps its `node` subscription only as a liveness signal | The Rust forwarder also projected that subscription and recovered it after a reset, without reading the projection; the Client owns the Node projection, and a forward ends when its subscription ends. Rust leaves a failed forward to the user; the window creates it again when the Host returns |
| The joining side does not check a pairing ticket's expiry | Rust rejects a ticket that its own clock finds expired or more than 60 seconds from expiry (`link/src/network/pairing.rs`, `rendezvous.rs`), so a correct ticket from a Host whose clock was 7.4 seconds ahead was refused. The sharing side enforces the lifetime and the token |
| Installation pairing is tried up to five times | Rust pairs once (`ssh/install.rs`, `wsl/install.rs`) and one early refusal left an installed Host unpaired |
| An update's unconfirmed result is released by Check status | Rust keeps offering the same request; a final unknown result cannot change, so the Host's version is read instead |
| Update confirmation drops the cached connection after each unconfirmed probe as well as before the first | Rust disconnects once before probing; a probe that reached the old process would otherwise leave its connection for the next |
| An SSH connection prefers its trusted host key's algorithm and sends `keepalive@openssh.com` every 15 seconds, closing after 45 seconds without a reply (`internal/ssh/connect.go`) | Rust connects with russh's default configuration: a server that added a key type the library prefers read as a changed key, and a connection whose route died stayed open. The order and the interval are OpenSSH's (`HostKeyAlgorithms` from known keys, `ServerAliveInterval` 15 with `ServerAliveCountMax` 3) |
| `HostInfo.timezone` is an abbreviation with its offset, not an IANA name | Go's standard library exposes the local zone's abbreviation and offset on every platform; the IANA name is not available without `TZ` or a platform call. The page no longer shows it |
| The sidebar footer keeps its summary samples | It shows the selected Node's load without the page's detail |
