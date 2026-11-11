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
| `menu.go` | What the rail's host menu opens (the port picker, a forward's address, Remove pairing) and the removal's confirmation |
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
the current host's page, and its Port mappings, Update and Remove pairing
items (`shell.App.hostMenu`, docs/shell.md) open the page with the port
picker (`State.OpenPorts`), the update dialog or the removal's confirmation
(`State.AskRevoke`). Add host opens Settings › Connections with the
Add host dialog (`settings.State.AddHost`). The header shows the host's
name and, over the metrics column, `metrics_title`; the details button and ⌥⌘B toggle the metrics (`App.Metrics`, the
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
Update host on a host updated from here and Remove pairing on a paired host
(`Actions.Revoke`; `menu.go`): its confirmation names the host and the
consequence once (`live_revoke_confirm`), and the button is disabled while
the removal runs (`Host.Revoking`). The live window binds it for every
paired host (`Runtime.revokeHost`, the `SetTrust` Settings › Connections
uses); a failure toasts, and a host whose pairing is removed leaves the
menu. A one-column description list
(type, projects, worktrees, sessions, terminals, active sessions), the
projects (rows opening the project's first conversation, or the dashed
empty card with Add project), the runtime's five facts with
`host_unknown` for unknown ones, the running sessions (agent or waiting
user, title, project / branch, model, status; a click opens the session).
The preview's tunnels section is not ported: the live host has none.

The metrics column (its title is in the header band, docs/shell.md, without
the chart icon): the preview note,
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
found once per `Client.Revision`, excluding delegated and archived sessions. Their navigation retains the original
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

The execution Node routes `stop_host_process` for its own machine, although the
accepted page has no control that sends it. It refuses plugin callers, PID 0,
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

A controller and a Host exchange execution calls only when their protocol
numbers and their releases are the same (`link.CheckHost`, and the Host's own
check of each call). Contracts stay at v1 and change in place until the first
release ([ARCHITECTURE.md](../ARCHITECTURE.md#contract-and-storage-versions)),
so the protocol number alone does not show that two builds agree. Releases are
compared by semantic-version precedence without build metadata. The older
side is the one to update and a release that cannot be read counts as older;
the `version_mismatch` fault names the side (`update_host`). A Host that does
not match stays offline with that fault as its runtime error and no Client is
started for it, so it is inspected every ten seconds instead of reconnecting;
an older Host shows Update, which works over SSH and does not need a
compatible Link. Two development builds of one release with different
contracts are not told apart.

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
| The dialogs, their drafts, the picker's query and choice, the failures already toasted, the chosen host, the metrics column | Everything in `Host` |

## Tests

`internal/desktop/host/host_test.go`: formats, the update's phases and stale
clicks, new forwards. `internal/desktop/shell/hosts_test.go`: the host page with
its metrics, the update dialog's Check, toggling the metrics, closing the
page; the rail menu's forwards, Update and Remove pairing with its
confirmation, cancel and busy state.

## Deviations

| Deviation | Reason |
|---|---|
| The page combines the preview overview (`workspace/host.rs`) with the live header's platform logo and forwards | Phase 6 asks for one host page on preview data; the live monitor (gauges, trends, process actions) needs a Node |
| Port forwards show on the preview page | The reference shows them only on a live host |
| The rail's host rows have no Update button; the picker's right-click menu offers Update for each host that has one | The approved picker rows are a name and a check; the reference's trailing Update would change them |
| The rail's Add host opens Settings › Connections | The add dialog lives with the Connections page |
| Choosing a host does not switch the sidebar's projects | The preview's sidebar lists every host's projects |
| Update Download prepares the update instead of acquiring a release | The frozen protocol has a single install/update command. Apply runs it: the remote machine downloads and verifies the release feed's installer and archive, or the execution Node uploads `host-artifacts/<goos>-<goarch>/` when that directory holds a locally built `sailry-host` |
| A port forward keeps its `node` subscription only as a liveness signal | The Rust forwarder also projected that subscription and recovered it after a reset, without reading the projection; the Client owns the Node projection, and a forward ends when its subscription ends. Rust leaves a failed forward to the user; the window creates it again when the Host returns |
| Compatibility also requires the same release | Rust compares only the protocol number (`link/src/compatibility.rs`), which it bumps with its contracts. Here contracts change in place at v1, and mismatched builds failed as "unknown Node event" and reconnected every ten seconds |
| The joining side does not check a pairing ticket's expiry | Rust rejects a ticket that its own clock finds expired or more than 60 seconds from expiry (`link/src/network/pairing.rs`, `rendezvous.rs`), so a correct ticket from a Host whose clock was 7.4 seconds ahead was refused. The sharing side enforces the lifetime and the token |
| Installation pairing is tried up to five times | Rust pairs once (`ssh/install.rs`, `wsl/install.rs`) and one early refusal left an installed Host unpaired |
| An update's unconfirmed result is released by Check status | Rust keeps offering the same request; a final unknown result cannot change, so the Host's version is read instead |
| Update confirmation drops the cached connection after each unconfirmed probe as well as before the first | Rust disconnects once before probing; a probe that reached the old process would otherwise leave its connection for the next |
| Remote timezone remains unknown | HostInfo and the Link handshake do not expose it; the controller does not substitute its own timezone |
| Live metrics keep the accepted column and sidebar footer | B7 supplies real samples without restoring the Rust monitor's additional gauges, trends or process actions |
