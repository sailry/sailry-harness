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
recovered and suppressed; it never launches old work. B6 owns automatic delivery
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
