# Files

Workspace file services live in `internal/node/files`; retained filesystem
capabilities and native Trash live in `internal/core`. The Node captures registered
worktree roots and access before releasing SQL leases. Every physical read,
stream and mutation runs outside SQL transactions and publication locks.

Directory pages sort directories first and retain at most 500 entries plus one
lookahead. Cursors bind the observed directory revision. Whole text reads return
at most 128 KiB and expose a full BLAKE3 revision only for complete content. Line
reads, file discovery and ignored-tree traversal reuse the core file owner.
Search supports literal and RE2 queries, content, files and counts, context,
pagination, strict text decoding and the vendored ripgrep default file types.
Unsupported or changing files produce bounded diagnostics and honest partial
results. Incomplete scans never resolve a later content offset.

Text edits resolve all exact matches before publication, preserve CRLF boundaries
and reject ambiguity or overlap. Change sets validate all targets before the
first effect, require one approval, retain exact prepared content and stop on the
first failed or uncertain effect. Already applied files remain applied; later
files are marked unattempted. Patches use the same prepared-change path and
require an observed full revision before changing an existing file. Notebook
creation assigns deterministic cell IDs; edits keep original index meanings,
preserve unknown fields and clear edited code outputs.

Entry copies retain confined source and target directories, reject links and
special files, enforce depth and entry limits, skip Git metadata on copies and
preserve it on whole-tree moves. Same-filesystem moves use native rename.
Cross-filesystem moves verify source and target identities, metadata, complete
children and BLAKE3 content before passing the source to Trash. Host picker
actions use absolute execution-host paths, protect home ancestors and report
per-entry conflict, replacement and cut outcomes.

## Host file picker

`internal/desktop/picker` is the in-app file picker of the reference
(`file_picker.rs`, `file_picker/**`): one `State` per window
(`sidebar.State.Picker`), drawn by the shell's overlays above every dialog.
It browses the file system of one execution host through a `Source`:
`Browse` (`browse_files`, paged until the cursor ends) and `Manage`
(`manage_files`). `live.Runtime.files(host)` binds the source to that host's
controller, so the local Node and a paired Host are read the same way and a
remote directory can be chosen; the preview uses the in-memory
`picker.Catalog`. There is no OS-native folder dialog, for the local host
either, as in the reference.

The dialog is Kit's with `DialogOptions{Bleed: true}`: the content runs edge
to edge under the frosted panel, 1040 by 680 at most and 24 DIP clear of the
window. The sidebar keeps the frost under its `Muted` wash; the header, the
entries and the footer sit on the `Popover` color at full opacity
(`kit.Opaque`), so nothing behind the dialog reads through them.

| Part | Behavior |
|---|---|
| Sidebar | 204 wide, flush left: the title and the host's name (`Options.Host`), then `directory_locations` (home, desktop, documents, downloads), `projects` (`Options.Projects`: the directories of the projects registered on the host, the most recently used first, eight at most, left out when empty) and `directory_devices` (volumes, roots). The location that holds the directory shown is marked, the longest one (`picker.Nearest`) |
| Header bar | Back and Forward over the dialog's own history, Up; the address as crumbs (the volume or root as the host names it, each crumb a menu of its child directories), scrolled to its end, or a path field after a click beside them or ⌘L; a spinner at its end while a directory is read; the filter; list and grid; New folder; More: the hidden-entries toggle, Refresh (F5), Go to folder (⇧⌘G) |
| Entries | `ui.Table` named `files` with Name, Size, Modified and Kind, sortable, directories always first; or `ui.GridView` tiles, a long name on two lines with a middle ellipsis and its tip; both virtualised. Each entry shows a tinted tile of its type (`picker.Class`), 20 DIP in a row and 44 in the grid. Hidden entries are dimmed. Modified is today's or yesterday's time of day, else a date, in the frame's language and hour cycle, the full time in a tip and a dash for none. Enter or a double click opens a directory (in the file modes, chooses that file alone and confirms), Backspace or ⌘↑ goes up, the arrows move the selection, typing finds an entry. `Files` mode chooses several entries through `ui.Selection` (Cmd-click, Shift-click, Shift with the arrows, Cmd+A); only those still shown stay chosen |
| Directory mode | Files show dimmed and cannot be selected (`picker.Selectable`): a click, an arrow or a double click that lands on one leaves the selection where it was |
| Empty | `files_directory_empty`, or `directory_no_matches` under a filter |
| Reading | A read blocks nothing: navigating elsewhere replaces it and its late answer is dropped. Confirm waits for it. With nothing shown yet, a spinner takes the entries' place |
| Failed read | Over a directory that shows: a failure toast with the reason, the directory and its choice staying. For the directory the picker opens at: the host's home instead, with `directory_fallback` naming the path (silent in the preview, whose sample tree holds no caller's path), or, the home failing as well, that directory's own reason. With nothing to show: the reason and the path in the entries' place with Retry, while the address, Go to folder and the sidebar (the home, the projects) lead elsewhere. The live source tells `directory_missing` (`not_found`), `directory_denied` (`permission_denied`) and `directory_host_unavailable` (`unavailable`, or no answer) from `directory_failed` |
| Menus (native) | Entry: Open, Copy, Cut, Rename, Paste (stop, replace, keep both), New folder, Move to Trash after a confirmation naming the entry; background: New folder, Paste, Refresh. Outcomes are toasts naming the host |
| Footer bar | What Confirm would choose: in Directory mode the path of the selected directory, otherwise of the one shown; in File mode the file's path or `directory_nothing_selected`; in Files mode one path, or `directory_selected_files` for several. Then the counts (`directory_counts`), Cancel and Confirm, which carries the choice as its description and tip. Confirm is disabled without a choice, while a directory is read, while the caller saves and once the caller's draft changed (`directory_stale`) |

A narrower dialog gives up, in this order: the Kind column under 960 DIP of
dialog width, the Modified column under 860, and the sidebar under 780, where
the title and the host move to a row over the header bar and the locations
into submenus of More. The window's least size, 760 by 560, leaves a 712 by
512 dialog with Name and Size.

List or grid and the hidden toggle are the controller preference
`file_picker` in `desktop/preferences.json`.

Callers, the reference's and ours:

| Reference caller | Mode | Go |
|---|---|---|
| Project editor's folder button | Directory on the editor's host | `sidebar.State.browse`; fills the path and suggests the name |
| Open folder (`menu_open_folder`) | Directory on the shown host | `sidebar.State.OpenFolder`; opens the registered project or adds one, the picker staying open while it saves |
| Settings › Backup, local directory | Directory on the shown host | `live.Runtime.chooseDirectory` through `settings.BackupActions.ChooseFolder` |
| (ours) Settings › Plugins, Install: package path | One file on the controller's machine | `live.Runtime.chooseFile` through `settings.PackageActions.Browse`: the package is read on this machine before its upload, so the picker browses the local Node whichever host is shown. It chooses a ZIP archive; a package directory is still typed |
| Documents plugin, Markdown image (`content_image_choose`) | One file | Not wired: the Go editor's image prompt (`files_insert_image`) takes an address only and has no Choose file button |

Everything else that takes a path in the reference (attachments, SSH keys,
imports, exports, package installs) uses the system's dialogs there and here.
The multi-file mode has no caller in the reference or here. Not ported, having
no caller: the Save mode.

Deviations from the reference, by the user's decision of November 11, 2026:
the rows take the look of CrychicUI's `files.FileList` without its module: a
tile washed at 14% with the status color of the entry's type (folders and
archives `Warning`, code `Primary`, images and sheets `Success`, PDF, audio
and video `Danger`, the rest muted) holding the glyph in that color, and the
chosen entry's name in bold. A chosen row is filled with the accent, so its
tile takes `PrimaryForeground`. The list keeps MyGo's own table and grid
behavior. The Modified column shows `protocol.FileEntry.modified_ms`, which
the Node's listing (`core.Files.ListDirectory`) and the SFTP listing fill
from the entry's own time; the field is optional, so an entry of a peer that
does not send it shows a dash. Several files are chosen with the
list's own modifiers rather than the reference's name-cell checkbox.

By the user's decision of November 11, 2026 the picker is laid out as a file
manager's window rather than the reference's padded dialog around a bordered
box: the full-bleed dialog above, the sidebar's Projects group, New folder in
the header, the rarer actions in More, the counts in the footer, and a read
that fails or hangs handled as the table says. The trash action is named
`directory_trash` (Move to Trash): `manage_files` moves the entry to the
host's trash and deletes nothing. `browse_files` answers `not_found` and
`permission_denied` for a path that does not resolve
(`internal/node/files/browser.go`), where it answered `unavailable`.

## Transfers

One `Transfers` instance owns raw authenticated streams. A stream is caller-bound
and opens once. Uploads verify exact size and BLAKE3, sync protected staging,
receive write-half EOF and acknowledge readiness before a durable finish
publishes a retained target. Downloads retain their verified open source and
Client verifies received bytes. Preparation, opening and idle limits are 8, 60
and 30 seconds; injected clocks support deterministic tests. Cancellation cannot
undo publication.

A stored attachment file without an `attachments` row is collected: at Node
start before any command is admitted, at shutdown after the transfer workers
drain, and at once after `discard_attachment`. A
finishing upload holds the Node's effects lock from publishing its file to
committing its row, and a collection during operation holds the same lock, so a
file is never taken between the two. Only ordinary files whose name is a
canonical attachment ID are removed.

Attachment IDs determine storage names; display names never become paths.
SQL binds immutable ordered references to turns. Reads from agent execution
require visible history membership, including joined guidance, and exclude
pending or active unrelated origins. Discard requires the staging caller and
refuses any attachment already bound to a conversation. Admitted commands receive
verified read-only copies under one private scratch directory, named only by
attachment IDs. `SAILRY_ATTACHMENTS` names that directory. Process cleanup releases
only its retained scratch identity and preserves the immutable attachment source.

Checkpoints retain the exact prepared before and after content before an approved
mutation dispatches. Their receipts come from the durable request ledger, not
filesystem inference. Turn diffs join adjacent confirmed versions only when their
content matches, preserving gaps caused by external edits. Hunk reject uses exact
whole-line context and a current revision; hunk stage uses the Git owner's one
index writer with captured working, index and HEAD revisions. Restore refuses
active turns and absent outcomes, and verifies the written after revision. A
file that already holds what it had before the change, or before an earlier
change of the same turn, is answered as restored and left untouched; any other
content is a conflict. Rust refuses the repeated restore as a conflict; the
changes card remembers an Undo only while its conversation stays open, so a
repeated Undo has to succeed rather than report a conflict that does not exist.

A checkpoint is captured in the transaction that admits its request, so a
replayed request is answered from the ledger before capture and an approval that
already has a checkpoint is a `conflict`. Rust captures in a separate
transaction and therefore accepts a repeated capture with identical content.

Checkpoint text and attachment bindings live as long as a remaining session
lists their turn; removing the last such session deletes them, the attachment
rows that lose their last binding and then the attachment files
([node.md](node.md#current-commands)).

The accepted native Files layout, view-model fields and Actions remain the UI
contract in [panel.md](panel.md). The coordinating Node owns shared native
Files/Repository watches, scope authorization, durable admission, approval
binding, publication ordering and live adapters. This package does not mount a
plugin-specific Files desktop surface.

Conversation inventory pages derive from canonical conversation events. They
retain visible fork membership, exact inline and tool image descriptors, captured
worktree roots, explicit attachment and file references, standalone Markdown
deliveries and published file or document content. Ordinary workspace-write
receipts do not become output files. Each read scans at most 256 events and
4 MiB, with 1 to 50 groups and an exclusive session-bound sequence cursor.
The inventory adds no filesystem scan, business table or second file index.

Expanded tool details derive a successful single-file edit from its captured
checkpoint, matching the reference detail reader. Later disk changes cannot
alter that display. Change sets and notebooks retain their declared result;
failed calls never acquire a successful diff. Unified file headers and hunk
metadata do not contribute to the conversation's added/deleted line counts.

## File watches

`internal/node/fswatch` is the one file watcher of the Node. The workspace
watch (`internal/node/workspace_watch.go`) and the knowledge worker use it;
the code index subscribes to the workspace watch.

| System | Backend | Cost |
|---|---|---|
| macOS | One FSEvents stream per watched root, through purego, on one serial dispatch queue | No descriptor per file or directory |
| Linux | fsnotify (inotify): the tree is walked and every directory added, new directories as they appear | One inotify watch per directory, bounded by `fs.inotify.max_user_watches` |
| Windows | fsnotify (`ReadDirectoryChangesW`), added per directory like Linux | One handle per directory |

fsnotify's kqueue backend is not used on macOS: it opens every file of a
watched directory. Measured on macOS 27.2 with fsnotify v1.10.1, a tree of
20,000 files in 200 directories held 20,205 descriptors and took 0.58 s to
add; the FSEvents stream holds none and starts in 3 ms. The limit of the
process is `kern.maxfilesperproc` (184,320 on that machine), shared with the
databases, terminals and child processes of the Node.

- Workspace watch: one watcher per worktree root, at most 32 roots, covering
  the root and the Git directories outside it (a linked worktree's Git and
  common directories). It opens with the first subscription of a root, outside
  the owner's lock, and closes with the last one or when the worktree is
  removed. Subscribers are the Files and Repository topics, the plugin
  repository status and the code index; the code index gives its subscription
  up after ten minutes without a query.
- Events: FSEvents coalesces for 100 ms; each subscription then waits 200 ms
  after the first change before it publishes `files_changed`. The Git filter
  decides whether a batch matters to the repository.
- Dropped events: when FSEvents reports dropped events, or more than 4,096
  events wait for the reader, the batch becomes one event that names the
  watched root, which every consumer treats as "everything may have changed".
- Failure: a root that cannot be watched answers `Unavailable` "file watch is
  unavailable". On Linux that includes an exhausted inotify watch limit; there
  is no polling fallback for the workspace watch. The knowledge worker rescans
  every five minutes while its watch is unavailable, and the code index marks
  its results partial.
- Symbolic links are not followed. On macOS reported paths are mapped back
  below the root as the caller spelled it.

Deviation from the reference: none in behavior. The reference uses the
`notify` crate, which is FSEvents on macOS and inotify per directory on
Linux; the Go port first used fsnotify on every system.

Tests: `internal/node/fswatch` (`TestWatcher`: tree and single-directory
events, descriptors held for 2,000 files, 256 roots) and
`internal/node/workspace_watch_test.go` (`TestWorkspaceWatch`: shared watch,
lifetime, concurrent open, removal). The Linux and Windows paths are compiled
and vetted with `GOOS`, not run.
