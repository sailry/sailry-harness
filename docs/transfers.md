# Transfers

The installation requests the rail shows and the Save as dialog
(sailry-public `installations.rs`, `installations/{popover,rows}.rs`,
`downloads.rs`, `downloads/transfer.rs`).

## Package and files

`internal/transfers`

| File | Holds |
|---|---|
| `model.go` | **The view model**: `Queue` (`Remove`, `Retry`), `Entry` (`Kind`, `Status`, `Label`, `Measured`, `Measure`, `Terminal`, `Active`), `Overall`, `Download` (`DownloadStatus`, `Key`) |
| `installations.go` | `Visible`, `Button` (the rail's button, its ring and its popover), the request rows |
| `download.go` | `DownloadDialog` |

Preview data: `preview.Installations()` (a plugin downloading with its
speed, a language package counting files on this app, a queued model, a
failed and an uncertain request) and `preview.PlayDownload`, which moves a
transfer along the frame clock (`internal/preview/installs.go`).

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
icon (package, processor, code), the title over `phase · where` (`… ·
Disconnected` while offline), a 2 DIP bar while running, the measure
(`45%`, `3/10`), a clock while queued, Remove on hover and Retry for a
retryable one; its tooltip is the title, the status with speed and the
error.

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

## Tests

`internal/transfers/transfers_test.go`: labels, measures, the overall
progress, Remove and Retry, the download's status line.
`internal/shell/transfers_test.go`: the rail's popover lists the requests
and Remove drops one; Save as plays to its end and Close shuts it.

## Deviations

| Deviation | Reason |
|---|---|
| The ring is drawn | MyGo's Progress is a bar and its Spinner has spokes (`ui/status_ring.rs` draws it too) |
| No save panel before Save as | The preview writes nothing; it names `~/Downloads` |
| The popover opens beside the button's top | MyGo's popover attaches by edge; Kit's opens centered on the button |
