# Transfers

B8 fills the installation rail from each authenticated Node's `installations`
topic through Client. Failed and uncertain requests remain until the Node accepts
Dismiss. A known failed request may be submitted again by an explicit Retry;
uncertain Retry inspects the original request and keeps its identity. Entries
from other controllers are visible but have no fabricated retry command.
This follows the source's optional domain retry capture: observation alone does
not grant authority to reconstruct or replay a command. A retryable uncertain
request keeps its original identity until inspection yields a known outcome.
Progress, phase and speed come from the Node queue; no preview timer runs in
live mode. The transfer `Queue` adds optional dismiss/retry callbacks while its
preview behavior and existing fields remain intact.

The installation requests the rail shows and the Save as dialog
(the reference implementation `installations.rs`, `installations/{popover,rows}.rs`,
`downloads.rs`, `downloads/transfer.rs`).

## Package and files

`internal/desktop/transfers`

| File | Holds |
|---|---|
| `model.go` | **The view model**: `Queue` (`Remove`, `Retry`), `Entry` (`Kind`, `Status`, `Label`, `Measured`, `Measure`, `Terminal`, `Active`), `Overall`, `Download` (`DownloadStatus`, `Key`) |
| `installations.go` | `Visible`, `Button` (the rail's button, its ring and its popover), the request rows |
| `download.go` | `DownloadDialog` |
| `entry.go` | `EntryTransfer`, `EntryTransferDialog` for captured copy/move entries |

Preview data: `preview.Installations()` (a plugin downloading with its
speed, a language package counting files on this app, a queued model, a
failed and an uncertain request) and `preview.PlayDownload`, which moves a
transfer along the frame clock (`internal/desktop/preview/installs.go`).

## Entry point

`shell.App` keeps `installs` and draws `transfers.Button` in the rail
between the bell and the host picker while there are requests (as
`installations::visible`). A file card's Download
(`conversation.Download`) calls `App.startDownload`, and the overlays draw
`DownloadDialog`.

## Layout

The button: 36 DIP ghost, a 16 DIP download arrow; while requests run the
arrow is 12 inside a 20 DIP ring (1.5 DIP, the progress color, the mean of
the measured requests, turning while none is measured); a danger dot while
one failed or is uncertain. The popover: 320 wide to the right of the
button, `installation_title` in a 40 DIP heading, the rows up to 55% of the
window (400 at most), or the empty state. A row: 56 high, a 28 DIP tile
(muted; danger or warning at 10% for failed and uncertain) with the kind's
icon (package, processor, code), the title over a 2 DIP progress bar (the
measure; turning while running unmeasured; no status or measure text),
a clock while queued; on hover Remove, and Retry for a retryable one,
float at the row's right end, vertically centred, taking no room. No
tooltip; the status (`phase · where`, `… · Disconnected` while offline),
the measure with speed and the error are the row's accessible name.

The Save as dialog: 460 wide, the file over its destination, the progress
bar (unknown while preparing and publishing, none after a failure), the
step while it runs, then Cancel, disabled once canceled or while
publishing, and Close when finished.

## Actions

| Action | Preview |
|---|---|
| Remove | removed from the queue |
| Retry | starting again, preview_notice |
| Download a file | the dialog plays preparing, receiving and publishing; finishing toasts preview_notice, nothing is written |
| Cancel, Escape | canceled: the info toast files_download_cancelled |

## Wiring a backend

Fill `Queue` from `installations::Queue` (the Node's and the controller's
installation views); Remove and Retry become the queue's remove and retry.
`Download` follows `downloads/transfer.rs` `Status` from the watch
channel; before it, ask for the destination with the system's save panel,
and on finishing toast files_download_done or the error key.

## UI state vs backend data

| UI state | Backend data |
|---|---|
| The popover's openness, the dialog's openness | `Queue`, `Download`'s status and sizes |

## Live downloads

`live/downloads*.go` connects the accepted download dialog to Client-verified
Node streams. Native Save as chooses a destination before transfer. Staging is
private and adjacent to that destination; publication syncs bytes and checks
the observed target and parent identity. Cancellation and conflicts preserve
existing content and never publish partial bytes. Open external downloads into
a controller-owned temporary directory that is removed on failure or shutdown.
Remote execution paths never pass to local Reveal.

`live/attachments_local_test.go` groups source evidence/limits, destination
conflict/cancellation/publication and canonical descriptor coverage. The
coordinator runs these with the Live category; they require no native window.

## Tests

`internal/desktop/transfers/transfers_test.go`: labels, measures, the overall
progress, Remove and Retry, the download's status line.
`internal/desktop/shell/transfers_test.go`: the rail's popover lists the requests
and Remove drops one; Save as plays to its end and Close shuts it.

## Deviations

| Deviation | Reason |
|---|---|
| The ring is drawn | MyGo's Progress is a bar and its Spinner has spokes (`ui/status_ring.rs` draws it too) |
| No save panel before Save as | The preview writes nothing; it names `~/Downloads` |
| The popover opens beside the button's top | MyGo's popover attaches by edge; Kit's opens centered on the button |


## Native entry transfers

The core Files panel uses `EntryTransferDialog` for copy, cut/paste and drag-move.
The form retains its source worktree/path, destination draft and remaining source
paths. A ready entry starts only through Paste or Move. Its immutable command
captures both source and destination WorktreeIDs. Confirmed completion advances
the queue; failures keep the current draft. Check result reuses an uncertain
command's original durable request. A repeated matching operation reopens retained
state and never reapplies confirmed entries. Cut clears only after every entry
confirms. Outcomes appear once as toasts.

The form uses the source's 508-wide Kit layout, source and destination groups,
remaining-path scroll, editable ready destination and existing footer controls.
It has no fabricated byte counts: `CopyEntryTo` and `MoveEntryTo` expose a durable
command result, so the active indicator is indeterminate. Cancel remaining stops
future admissions while the current admitted effect settles. Closing retains its
uncertain identity. Streaming uploads/downloads retain their own real progress
and cancellation boundaries. One modal is admitted at a time through the live
transfer owner; installation entries remain separate.

This additive form fills a verified UI gap: the accepted Save as model describes
a byte-stream download and cannot represent an editable copy/move destination,
a remaining entry queue or explicit outcome inspection. The original download
model, renderer and layout remain unchanged.

## SSH transfers

`live/plugin_native_ssh_transfers.go` implements the `sailry/ssh-transfers`
plugin module. Uploads stage on the execution Node and publish once; downloads
publish atomically to the chosen local file. At most two uploads run together
and each file is limited to 1 GiB. Scripts receive job snapshots only: local
paths, streams and credentials stay in the application. An unconfirmed
publication keeps its original request and is resolved by an explicit check,
never by repeating the transfer. Jobs appear in the owning plugin's list and
use the ordinary failure, conflict and uncertain toasts.
