# Managed commands

The Node owns commands in `internal/node/process`. Desktop and Host share the
same service. Controllers observe commands through Client and never own native
process handles. Closing a controller does not stop background work. B10 owns
terminal panes; this service does not add terminal UI.

Admission captures the running task turn, worktree root, attachments, environment
and access choice before launch. `Start` owns explicit background commands;
`ExecutePromotable` owns foreground commands and returns either the original
result or a receipt transferring that same process to session ownership. The
Node commits background evidence together with the started receipt; a
completion gate prevents an immediate native exit from overtaking registration.
The completion callback commits bounded output and outcome before the service publishes a
terminal status. Uncertain persistence never replays a process.

`List`, `Read`, `Page`, `Wait`, `Write`, `Stop`, `StopAll` and `Subscribe` supply
the existing v1 command contracts. Output pages use original byte offsets and
reject split character boundaries. Captures retain 64 KiB per stream; files
retain a 1 MiB prefix and bounded tail within 4 MiB per stream and a 256 MiB Node
budget. Finished output is evicted before active output. The service retains at
most 128 command entries where finished entries can be reclaimed. Background
capacity is 32. Announced loopback URLs are extracted from complete output lines
and remain candidates until B7's port forwarding resolves them.

The startup environment captures the system account's login shell once, with a
five-second bound. Commands reuse the frozen, filtered values. Workspace access
on macOS uses trusted `sandbox-exec`; Linux uses trusted `bwrap`. The policy
grants workspace and private scratch writes, safe toolchain reads, read-only
attachments and captured Git metadata, and denies networking. Nested read-only
paths pin their writable ancestors. Git's existing managed-boundary callback
prepares `.sailry/worktrees` and preserves `info/exclude` through one owner.

Every Unix command owns a process group. Windows owns a kill-on-close Job and
assigns the suspended root before resuming it. Descendants are terminated after
root exit as well as cancellation. Stop permits the source two-second grace on
Unix; Node closure kills immediately. Native PTY access uses the approved public
creack fallback on Unix and ConPTY through x/sys/windows. The PTY's exact device
is granted on macOS. Windows pipe output recognizes UTF-8, BOM-marked UTF-16 and
the captured console/OEM code page while retaining original offsets.

SQL-only registration, completion and recovery live in
`store/workspace_process_records.go`. Recovery marks interrupted commands as
recovered and suppressed; it never launches old work, and the service kills
the process groups a crashed Node left behind (see
[Crash recovery](#crash-recovery)). B6 owns automatic delivery
and background scheduling. `workspace_process.go` binds command routing and
captured hook launches. `workspace_runtime.go` persists completion before
publishing committed background tasks to conversation feeds. Client owns the
shared command subscription and output observers used by the composer capsule.
Canonical command tool results retain their original command identity. The
SQL-only tool-output resolver checks caller, visible history and exact result
revision before Node pages a retained stream through the existing Process owner.
Tool results freeze bounded retained-output metadata at return, including actual
byte totals, character-aligned retained ranges and an 8 KiB recent tail. Background
snapshots additionally include an 8 KiB head; foreground results reuse their
canonical prefix capture. Transcoded streams keep original byte offsets.

## Session lifetime

Removing a session dismisses its running and pending background evidence in the
removing transaction, for the session and every delegated child, and after the
commit the Node calls `Service.CloseSession` for each removed id: every command
of that session is killed, awaited and dropped from the service. A late
completion finds the dismissed row and records nothing. This matches the
reference.

Archiving leaves commands running, as the reference does: an archived session
can be restored with its work intact, and removal is the action that ends it.
The desktop closes an archived conversation's terminals because those belong to
its panel; commands belong to the Node.

## Outcomes and deadlines

A command's exit status is the root's. When the root has exited but a
descendant still holds an output stream, the two-second drain ends the read,
the exit status and outcome are kept, and each stream that was still open is
flagged `truncated`. The reference reports such a command as `unknown`; the
status is known, so only the output is marked incomplete.

A foreground command runs under its `timeout_ms` (120 seconds by default). Once
it is promoted to the session it is background work: the deadline is cleared
and only `Stop`, session removal or Node closure ends it. A command that is
never promoted still times out.

PTY commands start with `TERM=xterm-256color` and a 24 by 120 window, as the
reference does. The Windows ConPTY path sets the same values and is compiled
only; it has not run on Windows.

## Stall monitor

The service watches every background command, including a promoted one. When
neither stream has produced output for `StallWindow` (15 minutes) it calls
`Options.Stalled` once with the snapshot and the silent time. New output re-arms
the watch, so a command that stalls again is reported again. The watch is a
standard timer taken from `Options.After`; tests drive it with a clockwork fake
clock.

`store.StallCommand` records the report as a pending `stall:<command id>`
background row beside the running command row, and the Node schedules the
existing background delivery for it. A command that finishes settles its
unread stall row in the completing transaction, so a session is told once that
a command went quiet and not again after it ended. A stall never stops the
command.

## Stopping a session's commands

`Service.StopAll(session)` is the call for a caller that must end a session's
background work and keep the session: it signals every running background
command of that session, returns their current snapshots without waiting, and
the commands settle as `stopped` through the normal completion path. Finished
entries stay readable and the session can start new commands at once. It holds
no store transaction and is safe to call from a committed-change callback. The
`stop_commands` command is this call. A running foreground command is not
background work and is left to its turn, which cancels it.

`Service.CloseSession(ctx, session)` is the removal variant: it kills every
command of the session, waits for them and drops the entries.

## Sandbox setting

With `sandbox_commands` off, `run_command` runs unrestricted whatever `access`
the model passed, as the reference does: the setting is the user's, and a
model argument does not turn the sandbox back on. With it on, a missing
`access` is `workspace` and an explicit one is kept. Each command reads the
setting when it is prepared and again when its approval is recorded, so a
change applies to the next command of a running turn.

Approval of `run_command` on macOS and Linux
(`store.ConfigureWorkspaceApproval`):

| Setting | Access | Ask and Project | Full |
| --- | --- | --- | --- |
| on | `workspace` | approved by the sandbox, no prompt | same |
| on | `unrestricted` | the user decides each command; nothing is remembered | approved |
| off | `unrestricted` | an ordinary approval: Allow, Allow for the session, or Allow a prefix | approved |

On Windows there is no sandbox and every command is an ordinary approval.

The last row differs from the reference, which asks for every command once
the sandbox is off and offers nothing to remember. Turning the sandbox off is
the user's decision that commands are not confined; asking again for each one
adds no protection and only leaves Full permission as the way to work. A
session or prefix rule recorded while the sandbox is off never lets a command
out of a sandbox turned on later: with the setting on, an unrestricted command
is always the user's own decision.

A prefix rule answers for the command that runs. The rule is offered and
matched on the model's arguments, and a handler script may prepare the command
from them, so the prepared command is matched again before it is admitted
(`store.ApprovedWorkspaceRequest`): one outside the prefix is refused with
`prepared command is outside the approved prefix`, as in the reference.

## Crash recovery

While a Unix command runs, the service keeps
`<profile>/tmp/command-group-<request id>`, holding the process group and the
leader's start identity (the kernel start time on macOS; the boot id and
start tick on Linux). The file is removed when the command ends. `New` reads
the files a crashed Node left: a group whose leader is still alive with the
same start identity is killed, anything else is a reused pid and is left
alone, and the file is removed either way. A group whose leader has exited
while descendants survive is not killed, because nothing ties the pid to the
recorded command any more.

Windows needs no record: the Job is kill-on-close, so the operating system
ends the commands when the Node process dies. The Linux identity read is
cross-compiled and vetted only.

## Controller commands this desktop does not send

Checked against the reference desktop:

| Command | Reference desktop | This desktop |
| --- | --- | --- |
| `claim_terminal` | "Take control" on a terminal another controller owns | Sent the same way, see [panel.md](panel.md#b10-live-terminal-tabs) |
| `stop_commands` | Sent when a conversation pane is closed | Not sent. Closing a pane only removes it from the split ([shell.md](shell.md)); commands are stopped one at a time from the running-commands tray, and removing the session stops them all |
| `set_session_order` | Sent after a sidebar drag | Not sent; the sidebar's manual order is pins only ([sidebar.md](sidebar.md)) |
| `read_git_head` | Read for the repository chip | Replaced by `inspect_git`, which carries the head with the status the desktop already reads |
| `disconnect_connection` | No sender: only the result mapping in `settings/connectors/commands.rs` and tests name it | No sender; `feature_usage.go` keeps the mapping for a controller that does send it |

The Node still serves all five for other controllers.

Commands with no sender in the desktop's own pages, and who does send them:

| Command | Reference | Here | Verdict |
| --- | --- | --- | --- |
| `read_code_index_settings`, `save_code_index_settings` | No controller sender; enabling the `code` package saves the setting | Settings › Conversation's Code index switch ([codeindex.md](codeindex.md)) | Keep |
| `cancel_code_index_query` | Client sends it for an abandoned code search, outline or graph | The plugin host sends it when a view closes on `searchCode` or `outlineCode` ([plugins.md](plugins.md#desktop-wiring)) | Keep |
| `graph_code`, `read_git_show`, `read_git_blame` | Agent tools `code.graph`, `git.show`, `git.blame` and Node plugin callbacks | The same; no controller sends them | Keep, Node-internal |
| `read_turn_capture` | Client helper used by Node tests; allowed to Node plugin callbacks | Allowed to Node plugin callbacks; no bundled package calls it | Keep |
| `report_diagnostic` | Sent for a failed plugin panel, a failed plugin metadata read and a failed Git head inspection | Not sent: those failures store no system notification | Sender missing (plugin views) |
| `update_knowledge_collection`, `pause_knowledge_collection` | Node tests only; the plugin SDK prepares creation, rebuild and removal | The same | Keep; editing and pausing a collection has no page on either side |

### Removed commands

Removed from the v1 contract on 2026-11-12 because no controller, package,
plugin callback or tool sent them on either side; only tests did:

- `put_credential`, `revoke_credential` and the `credential` output. A key is
  stored by `save_provider` and a sign-in by `complete_provider_login`;
  `list_credentials` and `credential_changed` stay.
- `set_defaults` and the `defaults` output. The snapshot still carries
  `defaults` and a session created without a configuration still reads the
  stored row; nothing writes it, as before. `defaults_changed` stays declared.
- `bind_connection_session`. A connection conversation is created bound, by
  `create_session` with `config.resource`.
- `validate_provider`, the `provider_validation` output and its
  `ConversationDiscoveryValidation` value. `discover_models` stays.
- `read_catalog_status`. The snapshot carries `model_catalog`,
  `model_catalog_changed` follows it and `refresh_model_catalog` still answers
  with the `catalog_status` output.
- `read_file_checkpoint`, the `file_checkpoint` output and the response-size
  refusal only it made. `list_file_checkpoints`, `read_turn_diff`,
  `read_turn_hunks` and `restore_file_checkpoint` stay, and a restore still
  reads the checkpoint content inside the Node.

The feature counters `settings.save.credential` and `settings.save.defaults`
went with their commands. A peer that still sends one of the seven is answered
as for any undeclared command ([node.md](node.md#values-from-another-build)):
a Node's Link answers the request with a `rejected` reply and a local
controller's dispatch fails, both with `invalid_request` "invalid command".
Neither is admitted, recorded or run, so `inspect_request` for it answers
`not_admitted`. A ledger row an earlier build kept for a raw
`put_credential` request is still digested at startup.

### Updates and changes with one end missing

Every topic, update kind and conversation change kind has both an emitter in
the Node and a consumer in the Client or desktop, except these:

| Kind | Reference | Here | Missing |
|---|---|---|---|
| Topic and update `python` | The Client's `watch_python` feeds the running `load_skill` row's preparation stage and the preparation list of Settings › Runtime | The Node emits it (`internal/node/python_service.go`); nothing subscribes | Consumer: a dependency being prepared shows no stage |
| Event `python_settings_changed` | Settings › Runtime reads again | The Client drops it; the page reads once per host | Consumer: another controller's index choice shows after a relaunch |
| Conversation change `revealed` | Published when a visibility operation shows a hidden message, which only the empty-response operation writes | The Client reduces it; the engine decides an empty response's visibility when it writes the record and writes no visibility operation | Nothing: no cause exists here |

## Deviations and validation

The required MyGo PTY import trial failed because its implementation is internal;
the plan's public PTY fallback is used without copying framework code. The
standard Go account API omits login shells, so a small purego adapter reads the
public `getpwuid_r` ABI. Darwin's process inspection uses x/sys to verify exited
members before accepting a denied signal as settled.

Windows has no filesystem sandbox, matching the reference behavior. Linux cannot
mount a future, nonexistent `.git` entry read-only; existing captured metadata is
protected, while macOS also denies future `.git` writes by path rule. Platform
policy fixtures are deterministic. Real process, sandbox and PTY checks use the
integration build tag; cross-compilation alone does not establish native behavior.

Workspace-access terminals: on Linux the terminal's program runs under the same
`bwrap` policy as workspace commands (`process.SandboxTerminal`), with private
state under the profile's `tmp` directory that is removed when the terminal
ends, and without zsh prompt marks because the wrapper is the launched program.
On macOS they are refused with `not_configured`: a `sandbox-exec` profile must
name the pseudo-terminal device, which the pinned MyGo terminal opens inside its
own launch and does not report. The reference sandboxes both. The Linux path is
cross-compiled and vetted only; it has not run on Linux.

The stall monitor and the cut-output outcome are tested with a fake clock and a
fake runner; drain, promotion, PTY size and crash recovery run real processes
under the integration tag on macOS. The Linux start-identity read and the
Windows ConPTY size and `TERM` are compiled only.
