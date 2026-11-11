# Right panel

B8's AI CLI launcher retains the Node terminal ID returned by the captured
`open_tool_terminal` command and selects its live panel tab. The authorized B10
terminal prerequisite uses the pinned MyGo terminal after its headless spike:
Node owns the PTY and one Mirror per subscription; Desktop uses a Conn terminal
and the library View. Each frame applies its source viewport before its bytes.
Input, source clear, resize and appearance use the existing authenticated Client
commands with caller ownership and exact terminal revision. Closing the view's
connection only detaches; the explicit Close terminal action closes the Node
resource. The panel's existing chrome and find bar remain Kit controls.

The optional v1 `TerminalScreen.vt` payload carries typed source viewport and
byte-array frames; existing cell-grid declarations remain intact. Raw input and
source clear are additive optional command fields. No second emulator or
controller-owned process was introduced. The minimal terminal service retains
the source's limit of 32 running/pending starts and shares same-ID startup
reservations without holding its registry lock across native work.

SSH terminal opening remains B10; live browser tabs, live terminal tabs and split
panes are described below. The explicit preview still draws its marked placeholder.
Headless macOS and authenticated Link integration results are coordinator-owned;
visible Kitty images, actual IME interaction and Windows ConPTY require their
named platform checks.

### B10 live terminal tabs

In live mode a terminal tab is the shell of one Node terminal and its tab ID is
that terminal's ID. `live/terminal_tabs.go` owns the window's inventory on the
root runtime; `live/terminal.go` owns the MyGo terminal of each tab.

| Action | Backend |
|---|---|
| New terminal (toolbar, launcher, +, ⌃`) | `create_terminal` in the selected worktree with unrestricted access, a new terminal and a new tab every time (the reference's `request_terminal`; Browser shows its open page instead). The prepared request ID is the terminal ID, so the tab shows before the Node answers; a failed creation takes the tab back and toasts |
| Terminal offered | Only where the conversation has a worktree on a connected Node (`panel.State.Worktree`, set each frame from the selected runtime's `workspace.selected`): a session, also one without a project, which has its private worktree, and an unsent chat in a project, a Git folder or a plain one. An unsent chat without a project has none: the launcher and the + menu do not list Terminal and ⌃` does nothing there (the reference's `launcher::Scope::offers`). A creation asked for all the same toasts `terminal_unavailable` |
| Toolbar folder | `TerminalInfo.Directory` through `TerminalActions.Folder` |
| Close terminal | `close_terminal`, or `close_ssh_terminal` for a terminal without a worktree; the tab leaves on success or `not_found`, any other outcome keeps it and toasts failure, conflict or uncertain |
| Closing the tab in the strip, ⌘W | Asks first (`workspace_close_terminal` and the tab's title, Cancel or Close), then Close terminal: the shell ends on the Node and the tab leaves when it has |
| Find | MyGo terminal search through `TerminalActions.Search` |
| Saved tab after relaunch | The first frame that shows it subscribes again; a terminal that is not running is sent `open_terminal` first, which restarts a stored shell under the same ID |

Each frame `terminalsFrame` ports the reference `prune`: a tab stays only while
a Node's snapshot has its terminal with a status other than `closed`. Tabs of a
Node that is not connected stay, a tab created by a command stays until its
terminal first appears in a snapshot, and a saved tab no Node has reported
leaves once the local Node and every discovered peer delivered a snapshot.
Tabs with the preview's numeric IDs name no Node terminal and leave in live
mode. A terminal whose tab is gone is detached, never closed. The tabs are
every conversation's (`shell.App.AllPanelTabs`): a terminal of a conversation
that is not shown stays attached and running, and its tab is there on return
([Tabs per conversation](#tabs-per-conversation)).

The reported failure, "operation failed" on Terminal in a new chat without a
project, was the window refusing the creation itself: with no worktree
selected `createPanelResource` reported an `unavailable` fault through the
generic `Report`, and no command reached the Node. The Node rejects
`create_terminal` without a worktree as well. The reference never offers a
terminal there and has no home-directory fallback, so neither does the
desktop.

### Tabs per conversation

The right panel belongs to the conversation, as the reference's
`shell/session_scope.rs` and `session_scope/panels.rs` (`park_session_panel`,
`sync_session_panel`, `promote`).

| Fact | Reference | Desktop |
|---|---|---|
| The panel's content, shown item, openness and width | Per conversation (`Key::Draft`, `Key::Session(node, session)`); a conversation that never opened the panel shows it closed | Per conversation ID: `shell.ConversationPanel{Tabs, Active, Open, Width}` for every conversation but the shown one, whose are `panel.State` and `App.PanelOpen`, `App.PanelWidth` |
| Browser pages | Per session or draft (`browsers.views` by `Owner`), promoted when the draft is created | The same: a page's tab is in its conversation's panel (`browserTab.conversation`) |
| Terminals | Per Node and worktree, in the split workspace; the Node owns them | A tab of the conversation that opened it; the Node owns the shell |
| Open files and diffs | Per Node and worktree | Per project (`panel.Project`, `State.Sync`), unchanged |
| An unsent chat becoming its session | `State::promote` moves the draft's panel to the session | `App.ReplaceConversationID` moves the record by the draft's exact ID (`renamePanel`), with `browserAdopt` for its pages |
| A session removed | `Event::Removed` closes its view | Its panel record is dropped and its terminals are closed on their Node |
| Across a relaunch | Nothing of the panel is kept | Each session's tabs come back (below) |

`App.MountPanel` (`shell/panels.go`) runs at the start of every frame, again
after the live sidebar is listed (`live.conversationPanelsFrame`) and after
the window's shortcuts: when the selection changed it parks the shown tabs
under the conversation left and mounts the selected one's
(`panel.State.Mount`). A terminal has one tab in the window: showing it in the
selected conversation takes it out of another's. `conversationPanelsFrame`
also moves a browser page an agent opened for a conversation that is not shown
into that conversation's panel, and removes the panel of a conversation that
is gone: an unsent chat no longer in the window, or a session no connected
Node lists unarchived once every Node answered. Each of its terminal tabs is
sent Close terminal first.

`ui-state.json` keeps the shown conversation's tabs in the existing
`panelTabs`, `panelTab` and `panelOpen`, names that conversation in `panelOf`
and keeps the others in `panels`. A live window does not keep an unsent chat's
panel, whose ID is the window's. A file without `panelOf`, from before, gives
its tabs to the first conversation shown.

Local terminals are a core feature, like Files and Git. The `terminal`
resource kind is core-owned in live mode: a package's declared renderer for it
is never mounted, its creation contribution and shortcut are never used, and a
package cannot open or title a panel pane. A terminal is the only pane
resource of the v1 SDK, so no pane owner remains: `pane()` answers `null`,
`openPane` answers `false` and the other pane calls report unavailable
(`bindPluginPanes` in `live/plugin_resources.go`). ⌃`, the panel's New
terminal (`createPanelResource`), the project's New terminal and ⌘D/⌘⇧D
therefore always take the core path, also with the bundled `commands` package
enabled. The rule names the resource kind, not a package. A package page may
still embed the native Terminal component for a terminal it opens itself; SSH
terminals stay package-owned that way. A saved tab whose ID is not a Node terminal ID, such
as an earlier `plugin/…` tab, leaves under the prune rule above.

The terminal display preferences are read in one place,
`Runtime.terminalDisplay`: font size and family apply to every open core
terminal as they change, and paste protection holds a paste of several lines
behind a confirmation (`terminal_paste_confirm`, `terminal_paste_warning`,
`terminal_paste`). A Terminal component embedded in a package page keeps its
own options.

Deviations:

| Deviation | Reason |
|---|---|
| While a terminal's split pane is displayed, its tab's screen is empty | MyGo shows a terminal in one view at a time; the pane owns the screen |
| Terminal tabs keep the title Terminal | The accepted tab title is unchanged; the Node title shows in the project's Terminal records |
| Terminals are tabs of the right panel, one strip per conversation, and several can be open (user decision 2026-11-11) | The reference shows terminals in the split workspace per worktree. Dragging a tab onto the conversation still splits |
| A removed or archived conversation, and an unsent chat that is discarded, end their terminals (user decision 2026-11-11) | The reference leaves a worktree's terminals to the Node. Here a terminal is reachable only through its conversation's tab |
| A terminal of an unsent chat is not listed after a relaunch | The chat's panel is not saved; the terminal keeps running on its Node and shows in the project's Terminal records |
| A session's panel tabs come back after a relaunch | The reference keeps nothing of the panel. The accepted window state already restored the tabs; it restores them per session now |
| A local terminal reports no activity (OSC 9;4, 133, 633) | MyGo's terminal view does not surface these sequences; SSH terminals report theirs |
| An exited shell closes its terminal, tab and panes (user decision 2026-11-10) | The reference prunes only `closed` or missing terminals. The desktop closes an exited terminal once; a failed one stays to show its error, and a stopped one stays so it can be reopened |
| A saved tab of a peer discovered after every known Node answered is dropped | A tab stores no Node; ownership is known only from snapshots |
| Local terminals are core; a package's `terminal` renderer, creation contribution and shortcut are not mounted (user decision 2026-11-10) | The reference lets a package own terminals. Here local terminals are core, like Files and Git; SSH terminals remain package-owned through the Terminal component of the package's own page |

The column right of the conversation: one flat strip of tabs, each an
open item, in the header band's segment over the column (docs/shell.md),
and under it the active one, or the launcher when none is open (the reference implementation
`resources/*.rs`, `navigation_tabs.rs`, `resources/launcher.rs`,
`preview_files.rs`, `preview_git.rs`, `hunks/render.rs`). The frame owns
its width, motion, toggle and ⌘W (`docs/shell.md`).

## Package and files

`internal/desktop/panel`

| File | Holds |
|---|---|
| `model.go` | `Kind`, `Resource`, `Resources`, `Tab`, `State`, and the rules `Opened`, `Closed`, `Overflow` |
| `project.go` | `Project`, `FileView`, `GitView`, `Projects`: open files and diffs per project; the split |
| `panel.go` | `View` and `Page`, the two hosts: the strip, the launcher, the file view, hunks, checkpoints, documents |
| `files.go`, `selection.go` | The file tree, its selection (the files plugin's `explorer.js`), keys, drag and right-click menu |
| `git.go`, `gitmenu.go`, `changes.go` | The Git component's side column (the git plugin's `view.js`), its menus and picker (`menus.js`), the change list's rows (`changes.js`), All changes |
| `tools.go` | The browser and terminal tabs' chrome, per tab: the browser's history and address, the terminal's folder and find bar; `State.Find` |

## Entry point

```go
s := panel.New(data.Workspace)
ps := panel.NewProjects(data.Workspace)
pr := ps.Of(project.ID)               // nil: no project, the hosts ask for one
panel.Strip(c, s, pr, shell.KeyText)  // the tabs, the chevron and "+", into the header's row
panel.View(c, s, pr, shell.KeyText)   // the column's body; KeyText writes the launcher's shortcuts
panel.Page(c, panel.Files, pr, files) // the page host, in the main column; files goes to the Files page
s.Open(panel.Files)                   // the launcher, "+" and the frame's ⌘P, ⌘T, ⌃`, ⌃⇧G
s.Show(panel.Tab{Kind: panel.Documents, ID: id}) // open or focus one item's tab
s.Show(panel.Tab{Kind: panel.Files, ID: path})   // a file's tab, opened in the project too
s.Close(s.Active)                     // ⌘W; false when no tab is left
```

## Components and hosts

Files and Git are each one two-column component drawn by two hosts:
`Page`, the rail's independent page in the main column, and `View`, the
right panel's tabs. Both draw the same layout (`resources/split.rs`): the
main area on the left and the side list on the right (the tree, or the
change list), 220 DIPs at first, dragged between 180 and 480 with the
main area keeping 180. The list is a fixed-width column past our own 6
DIP drag handle, not MyGo's `Split`: that sizes its first pane, and
converting the list's width from the last frame's layout drew a first
frame (a page switched to, a tab chosen, the panel opening) at the
minimum; the panel's motion moves only the main area.

Every open file and every open diff is its own item: three open files
and one diff are four tabs in the panel's one flat strip. The active
tab decides the content: its item in the main area, the tree beside a
file tab, the change list beside a diff tab. Choosing in the side list
opens or focuses that item's tab. The page heads its main area with a
strip of the same kind's open items. The items are the project's
(`Project`: `FileView` open files, each with its own Document/Source
and text scroll, the shown file, open folders, tree scroll and width;
`GitView` open diffs, each viewer with its own scroll, the shown
change, commit draft, list scroll and width), which `Projects.Of` makes
once per project ID: opening or closing an item on the page opens or
closes its panel tab, and the reverse (`State.Sync`, every panel frame
and before a shortcut opens a tab); the item chosen last in either host
is the one the other opens first; another project swaps the panel's item
tabs for its own. MyGo binds a `ScrollState` to one container per build,
so while the page and the closing panel both draw a resource
(`State.Paged`), the page owns its scrolling. Documents, browser,
terminal, subagent and file-preview tabs are single-pane.

## View model

| Type / field | Meaning |
|---|---|
| `Resource{Kind, Icon, Label, Mods, Key}` | A tab and launcher row; `Key` 0 for none |
| `Tab{Kind, ID}` | An open item: `Files` and `Changes` (by path; "" until the project names the item shown last), `History` (ID ""), `Documents` (by document ID), `Browser` and `Terminal` (by number), `Artifact` and `Subagent` (by ID); a saved tab of a kind no longer shown is dropped |
| `State.Tabs`, `Active` | Open tabs in order, the one shown (kept across launches by the shell) |
| `Project` | One project's open files and diffs and their state, shared by page and tabs |
| `preview.Workspace.Files` | `FileNode` tree: folders have `Children`, files `Text` |
| `preview.Workspace.Changes` | `FileChange{Path, Status M/A/D, Added, Removed, Hunks}`; `Hunk{Header, Lines}`; `DiffLine{Kind, Old, New, Text}` |
| `preview.Workspace.Checkpoints` | `Checkpoint{ID, Turn, Time, Files, Added, Removed}`, newest first |
| `preview.Workspace.Documents` | `Document{ID, Title, Path, Text}` (markdown) |

### Layout

- Strip: one flat row, no tabs inside tabs, built by `Strip` into the
  header band's segment: the tabs scroll and shrink, a spacer, then the
  chevron and "+" at the segment's end. `View` draws no strip. Without a
  tab the segment is empty. Tabs 28 high up to
  180 wide with the item's type icon and title (a file's or diff's name, a document's title); the active tab in
  `tab_active`; the close button shows on hover or on the active tab. It
  scrolls sideways; past `Overflow` (4) tabs a chevron (`navigation_tabs`)
  lists all with a check; "+" lists every resource with its shortcut and
  each document; Files and Git open the file or diff shown last. Opening
  an open item focuses its tab. Menus are `kit.MenuButton`. The pages'
  strips draw the same tabs, without "+" and the chevron.
- Launcher: `details`, then 40 DIP secondary rows up to 440 wide: icon,
  label, Kbd shortcut. Order: Browser ⌘T, Terminal ⌃`, Files ⌘P, Git ⌃⇧G,
  History, Docs. History and Docs belong to a session: the live launcher
  and "+" menu leave them out on an unsent draft (`State.Session`).
- Empty states: a tab with nothing to show draws `kit.Result` with its
  resource's icon, centered, never a blank body: History
  `panel_history_empty`, Documents `panel_documents_empty` (a spinner while
  the session's are read, `Actions.Loading`), Files and Git without a
  project `resource_project_empty`, Files without a file `file_empty`, a
  browser tab before its first address `browser_empty`, a preview or a
  project `resource_project_empty` (their pages add New project,
  docs/shell.md), Files without a file `file_empty`, a
  browser tab before its first address `browser_address`, a preview or a
  subagent that is gone `artifact_preview_failed` and
  `chat_child_unavailable`. Documents opened without a document is one tab
  that takes the first document once the session has one.
- Files: the tab's file in the main area, the tree in the side list, in
  the page and a file tab alike. The tree has no title and starts at 8
  DIPs (rows 28, 16 per level from 8). It selects as the files plugin's
  explorer: a click one row (a file opens, a folder opens or closes),
  ⌘-click (Ctrl elsewhere) adds or removes one, ⇧-click the rows from the
  anchor; the arrows move the selection, ⇧↑/⇧↓ extend it, → and ← open
  and close folders, ⌘A selects every row, Enter opens. Its right-click
  menu (the system's) acts on the selection, less rows inside a folder
  also selected; F2, ⌘C, ⌘X, ⌘V and a row dropped on a folder would write
  files. File view
  toolbar: Save, Undo, Redo (disabled; edits stay in the view), Find, and
  Document | Source for markdown (`kit.Markdown`'s `Document` variant:
  MyGo's markdown in the app theme, code blocks in MyGo's editor). Text in MyGo's editor (`kit.Code`, editable:
  highlighted, line numbers, mono 13 at 1.6, the caret showing, trailing line breaks left out). Status bar: path (right-click: Copy path),
  `Ln 1, Col 1`, UTF-8, language.
- Changes: the tab's diff, or All changes, in the main area, the git
  plugin's details in the side list, in the page and a diff tab alike:
  the branch (the checkout picker), the actions menu (`menus.js` root
  with its submenus) and Refresh; Changes | History (`kit.Segmented`,
  Block); Diff (All changes), the total `+N −M` and Stage or Unstage
  (`kit.SplitButton`); the change list, a tree grouped Tracked ·
  Untracked at first (or Staged · Unstaged, or none), as folders or flat,
  sorted by path, name or status, its rows the status letter in
  warning/success/danger, name, folder when flat, `+N −M` and a stage box;
  the commit box (options line, message with expand, Commit with its
  menu). A click or the arrows open a change's diff, which stays
  selected; a row's right-click menu acts on every change in it. The
  history lists the commits with their id, author and branch. The diff is MyGo's diff viewer
  (`kit.Diff`), unified as Rust's content diff view: highlighted, old and
  new numbers, word changes, and on the change under the pointer Revert
  and Keep (`editor.DefaultHunkActions`) and Stage. Status bar: path, `+N −M`.
- History: a card per checkpoint: the turn, its age (against the frame
  clock when live, the fixture's clock in the preview), the files, `+N −M`
  and `turn_changes_undo_all`.
- Documents: a tab per document, rendered.
- Artifact: a file's preview; a picture is `kit.Image` (a Skeleton while it
  loads) that opens `kit.ImageViewer`, whose Copy, Save as and Open toast
  `preview_notice`.
- Browser (`browser/view.rs`, `browser/shortcuts.rs`), per tab: Back and
  Forward through the tab's history (disabled at its ends), Reload
  (disabled before a first address) and the address (`browser_address`; Enter
  goes, `https://` is added when no scheme is given), over
  `resource_browser_preview` under the address shown. ⌘T (another tab) and
  ⌘R (reload) work while the focus is in the tab. The strip's "+" is the one
  button that opens a tab; the toolbar has none. The live tab's chrome is
  under [Live browser and documents](#live-browser-and-documents).
- Terminal (`workspace/terminal.rs`, `terminal/search.rs`), per tab: the
  folder it starts in (the selected project's path, muted), Find, Close
  (`workspace_close_terminal`) and New terminal, over
  `workspace_terminal_preview`. The live terminal has no Reset.
  ⌘F with the focus in the tab opens MyGo's `FindBar` at the screen's top
  right (420 wide at most, 0 matches in the preview);
  Escape or Done closes it. The window routes the native menu's ⌘F
  (`conversation.find`) to `State.Find` while the panel holds the focus.

## Actions

| Action | Effect in the preview |
|---|---|
| Open, choose and close tabs | `State`. A tab's close button, in the header band, and ⌘W both close it; the focus goes to the panel (`State.Focus`), where ⌘W closes the next tab. MyGo runs `OnClick` after the build, so the close acts in its callback |
| Choose a file or a change | Opens or focuses its tab, in the panel and the page alike |
| New terminal, the strip's "+" and ⌘T | Another terminal or browser tab |
| Back, Forward | The tab's history |
| Close (terminal) | Closes the tab |
| Find (terminal) | The find bar, nothing to match |
| Revert, Keep, Stage a change, Undo changes, Find, Reload, address | `preview_notice` toast: they write files, git, load pages or start a shell |
| Git: pull, push, fetch, commit, stage, discard (after its confirmation), branch, stash, tag and remote actions, Refresh; Files: open, copy, cut, rename, trash, new, move | `preview_notice` toast |
| Git: view options, commit options and amend, pickers, history selection; Files: selection, folders | Work in the view |
| Copy path | Clipboard |

## Live backend wiring

`internal/desktop/live/workspace_*.go` owns the projection and action adapters.
One `panel.Project` belongs to each real worktree. Switching sessions replaces
session-owned checkpoints and document metadata; switching worktrees releases
its Files and Repository observations. The Client shares topic subscriptions.
B0 preview projects leave `Actions` nil and retain their isolated behavior.

The tree reads bounded `ListDirectory` pages through Load more, refreshes the
visible extent with fresh cursors, preserves expanded folders and reads
an opened file with `ReadFile`. File references can open a nested file before
its folders have been expanded. Complete text with an observed revision is
editable. Save sends `WriteFile` with that revision. Refresh and failed saves
retain dirty drafts; a completed save also retains edits typed while the save
was in flight. Original hidden trailing line breaks remain exact. Undo, Redo
and Find use the existing MyGo editor state exposed by Kit callbacks.

The existing file menu sends `CreateDirectory`, `RenameEntry`, `TrashEntry`,
`CopyEntryTo` and `MoveEntryTo`. Clipboard provenance captures the source
worktree. Sequential batches stop on the first failure. A Trash confirmation
retains its confirmed position for a retry from that confirmation; the same
entries confirmed again start from the first one. Native open, reveal, download and reference insertion use
`WorkspaceNative`; execution-host paths never become controller-local paths.
The window's transfer owner verifies downloaded content and owns native effects.

Git status and branches project into the existing fields. A status read names
the first change as the one shown while none is (`GitView.First`) and adds no
open diff: a status read opens no diff tab, and a closed one stays closed. Existing menus send
revision-aware index, commit, branch, remote, stash, tag and worktree commands;
forms retain values after failures. Destructive operations name their target
in the existing confirmation. Pickers preserve full stash and tag identifiers.
Output, commits and stashes open in the existing document tabs. Diff scope caches
remain separate and invalidate with Repository updates. A native hunk's line
range maps to the displayed patch, then to a unique captured turn-hunk identity;
reordered or unrelated reviews cannot select an arbitrary hunk. Stage and Revert
send `ApplyTurnHunk` with file/index/HEAD expectations. Failed or uncertain Revert
keeps the authoritative content. Keep is native local review acceptance, matching
the protocol's lack of a separate persistence operation for Keep.

History reads final runs, including older conversation pages, and every
`ListFileCheckpoints` page. Counts come from `ReadTurnDiff`. Undo confirms the
selected turn, then restores checkpoint files in the source's newest-first order
and stops at the first failure. Each finished run's checkpoints are read once
and again only when its status changes, and the older pages once per session
load or Refresh. Session documents read `ListArtifacts` and
`ReadArtifact`, converting stored HTML to Markdown outside the UI thread. A
revision cache avoids repeated reads on each presentation frame. Outcomes appear
only in distinct failure, conflict, uncertain or success toasts.

Closing a view detaches presentation; explicit terminal actions own process
termination. Files, Git and local terminals are native core panel owners: since
B10 the `terminal` kind is core-owned, so the commands package's terminal
renderer and its creation contribution are not mounted and the panel has no
package pane hooks (see [B10 live terminal tabs](#b10-live-terminal-tabs)).

The plugin terminal bridge reuses the existing MyGo Mirror/Conn owner and
checks workspace read/control authority before creating a view. Disconnection
retains its native presentation while disabling interaction. General terminal
settings and split-terminal acceptance remain B10.

## Live browser and documents

A live Browser tab is a pinned MyGo `WebView` under the accepted chrome
(`panel/browser_live.go`, `live/browser.go`). Back and Forward follow the
page's history, Reload and ⌘R reload it and stay disabled before a first
address, and ⌘T and the strip's "+" open another tab, whose address field
takes the keyboard. The page is untrusted: it has no host function, opens no
window and stays on HTTP(S) addresses without embedded credentials. A
workspace plugin that declares a browser resource still creates it; the core
tab is the fallback. The WebView is created before the next frame builds and
is destroyed with its tab. Where the system has no WebView the tab shows
`browser_unavailable`.

The toolbar, left to right, as the reference's `plugins/browser` view draws it:

| Control | Behavior |
| --- | --- |
| Back, Forward | The page's history; disabled at its ends |
| Reload / Stop | Reload (`browser_reload`, ⌘R); while the page loads the same button is Stop (`browser_stop`, the close icon), which stops the load and keeps what the page has |
| Address | Enter goes to `panel.BrowserDestination`: text with `://` as typed, a host as `https://` (`http://` for `localhost`, `127.` and `[::1]`), anything else as a web search; nothing, `javascript:`, `file:` and `data:` toast `browser_invalid_address`. The field follows the page's address except while it has the focus |
| Select element | `browser_annotate`, ⌘⇧C; `browser_cancel_selection` while selecting. Enabled on a loaded HTTP(S) page only |
| More actions | A Kit menu: Web inspector (`browser_inspect`, ⌘⇧I, `Page.ToggleDevTools`) and Settings (the browser package's settings section) |

Page state (`panel.BrowserPage`, written by the WebView's events in
`live/browser.go`): `Loading` from `LoadURL`, Reload, Back, Forward or the
page's own navigation until the load finishes, fails or is stopped; an
indeterminate 2 DIP bar shows under the toolbar and a spinner stands for the
tab's icon in the strip. MyGo reports no load fraction, so the bar is not a
percentage. `Title` names the tab once the page has one. `Error` replaces the
page with a `kit.Result` (`browser_load_failed`, or `browser_unavailable` when
the WebView cannot be made) over the address with a Reload button: a failed
page is never a blank body, and it is not repeated in a toast. A cancelled
navigation (WebKit's -999) is not a failure. A load that ends without an event,
as a step inside one document, is reconciled with `Page.IsLoading`.

The WebView's own user agent gets the suffix `Safari/605.1.15 Sailry/<version>`.
In native UI MyGo otherwise sends WebKit's bare agent, which sites such as
Baidu answer with a blank page that ends "loaded" without a title or an
error: the white body reported for `https://www.baidu.com/`.

`browserSurface` places the page with `ui.WebView` as the rest of the tab's
column under the toolbar. MyGo moves the native view to that element's box on
every frame, so the page follows the panel's handle, the window, the panel's
slide and the display's scale; nothing in the adapter keeps a size. The
panel's resize handle lies over the first DIPs of the page's left edge and
keeps the pointer there.

Element selection (`live/browser_pick.go`, `live/browser_pick.js`, ported from
the reference's `annotation` module): hover highlights, a click picks,
Shift-click marks up to eight, Alt-click picks and ends the selection, the
arrows walk the tree, Enter picks and Escape cancels; the selection stays on until it is cancelled, the page
navigates, the tab closes, its conversation leaves the screen (the reference
cancels with its browser's view) or 120 seconds pass without a pick. The untrusted
page cannot call the app, so the script queues its messages and the adapter
reads each as the answer of an evaluation awaiting the next one. Every field
of a picked element is bounded and checked against the reference's limits
before use; the viewport capture is cropped to the element plus 8 CSS pixels
as a JPEG, and the page context is the reference's `browser_element` JSON,
marked as untrusted data and reduced to 16 KiB. Both go to the composer of the
tab's conversation as `browser-element-N.jpg` and `browser-element-N.json`
through `composer.State.Attach`, together or, over the attachment limit, not
at all. The tab's conversation is the one that owns its page
(`browserTab.conversation`, the panel the tab is in): a pick that arrives once
that conversation is no longer on screen toasts `browser_annotation_stale`,
ends the selection and reaches no composer.

Browser tabs share one website data store (`live/browser_settings.go`). On
macOS with `browser_persistent` on, the default, it is the MyGo session named
after the profile directory, kept on disk from macOS 14; otherwise, and on
every other system, it is one in-memory session of the process. When the
preference changes, open tabs drop their pages and load their addresses again
in the other store. The Browser settings page's Chrome import sets cookies in
this store ([settings](settings.md#browser-page-wiring)).

The agent's browser tool uses the same tabs. `internal/node/browser.go` routes
`use_browser` to the controller that issued it as a `browser_call` update on
the `browser` topic and settles it with `complete_browser`: one subscription
per controller, a 30-second deadline, a retryable fault for unanswered reads
and an unknown outcome for unanswered actions. The deadline is each call's
`expires_at_ms` and bounds every action: a `wait` may ask for a longer
`timeout_ms`, but it ends with the call as an unanswered read, and the timed-out
call leaves no pending entry for a late completion. `browser.read` and
`browser.control` plugin tools reach it through the turn's native tools; control
actions ask for workspace approval. `client.ServeBrowser` delivers each call
under a context that ends at the call's `expires_at_ms`, so a wait or a queued
action that outlives the call is abandoned (the Node has settled it by then)
and the next call is served; an action still queued for the UI thread when
its call ends does not run. `live/browser_calls.go` runs each call against the tabs opened in the call's
conversation with the automation script of the reference, never with source
the model provides. Those are the browser tabs of that conversation's own
panel, on screen or not: a call that names no tab goes to the one that panel
shows (`shell.App.ConversationActiveTab`), and a tab that has shown no page
yet, as one that came back with its conversation after a relaunch, belongs to
the conversation whose panel has it, never to the selected one for being
selected. Captures are measured, cropped and encoded within 1 MiB and
1568 pixels by `live/browser_capture.go`, and the Node checks them again.

A session document renders its own HTML (`live/document_view.go`,
`panel/document_live.go`) in a WebView with an ephemeral session, no preload,
no trusted origin and developer tools disabled. The HTML sits in a
`sandbox="allow-scripts"` frame of an opaque origin under a policy without
network access, and the page may only be `about:blank` or `about:srcdoc`. The
document tab keeps its converted text until the HTML is read or where no WebView
exists. The view is destroyed once a frame no longer places it.

`go run -tags integration ./internal/desktop/live/browserprobe` checks page
loading, the automation script, capture and the document sandbox on the
current platform against pages it serves itself, then a browser tab of the
real App: its history, the agent's calls, a slow page loading under Stop until
it finishes or is stopped, a wide page whose `innerWidth` is the body's width
through two resizes of the panel, an element picked in the page reaching the
composer, and a refused connection ending in the failed state. Its windows
must be visible: an occluded window draws no frame, and a navigation waits for
the next one. It ends by loading `https://www.baidu.com/` and printing how
that ended, as evidence when the network is there.

## Live file editing

`panel/editor_*.go` owns native editor state and controls. Text edits retain the
original file ending and stay local until the existing revision-checked Save
action succeeds. Tabs mark unsaved drafts; either host's Close offers Save,
Discard and Cancel. Saving holds the close continuation until the exact draft
is confirmed. A failed or uncertain save leaves the draft and tab intact. Failed
file refreshes hide stale clean content and preserve dirty content as read-only.

The accepted toolbar uses native undo/redo, Find and Replace, soft wrap, Markdown
block and inline formatting, lists, links and insert menus. Controls collapse in
the source's group order into More. Keyboard formatting uses the same actions.
The footer reads the native caret, language, encoding and saving/partial state;
its OS-native path menu also opens the captured workspace file externally.

Markdown documents use pinned MyGo `richtext.Document` where its parser and
serializer preserve the supported structure. Entering Document mode does not
rewrite bytes; undoing back to the original native document restores its exact
original source. Verified codec gaps include tables, language-tagged fences,
raw HTML, images, footnotes and multiline paragraphs. These documents keep the
existing Markdown preview and edit their original source. Unsupported block and
insert operations switch to Source while preserving unrelated content. This
avoids serializing an incomplete native model over the original file.

The rich/source selection bridge reads MyGo's public parser byte spans and
converts the native rune selection before a source insertion. Escapes, entities,
Unicode, nested lists, code spans and fenced code retain their source positions.
An entity that contains a selection boundary between decoded characters expands
only at that token, with the same rendered text. A changed draft cannot receive
an insertion calculated for older bytes.

`live/attachments_editor_notebook.go` reads revision-fenced Notebook pages and
projects cells, outputs and bounded PNG/JPEG images off the UI thread. Preview
shows Markdown, read-only code/raw cells and captured outputs without starting
a kernel. More cells retains the same revision; failures allow explicit Retry.
Unsaved JSON projects from the local draft, and invalid drafts return to Source
with their bytes intact. Asynchronous results cannot recreate a closed tab or
replace a newer draft. Notebook source saves use the ordinary full-file action.

Child tabs use the real child SessionID and the shared Client conversation feed.
Opening verifies the parent, child, project and worktree association. The live
shell supplies `Hooks.RenderChild`; rendering projects the child's canonical
history without copying a parent or preview transcript. The child composer
remains read-only, and closing its tab only releases observation.

## B7 execution ownership

Each execution Node has a bound live Runtime and its own workspace, attachment
and preview descriptors. The shared panel presents the selected worktree while
retained artifact tabs resolve their descriptor's original owner. Switching to a
local conversation therefore cannot send a remote preview's Open, Save or image
action through the local Client. Native file callbacks installed by the window
propagate to both existing and newly bound Runtimes.

The window presents one transfer dialog at a time, retaining the selected
transfer's Runtime until its confirmation or progress flow closes. Remote
copy/move confirmations, upload Replace decisions and attachment downloads use
their captured Node even when navigation changes. Dialog Start, Retry and Cancel
never dispatch through the newly selected conversation.

Hooks and Sandbox share the settings presentation, with reads and saves bound
to one retained execution owner. Per-Node and per-worktree drafts preserve
revisions and failures. An open hook editor or consent dialog keeps its original
owner through navigation; a remote read cannot populate a local save callback.
Execution-host paths remain remote resource identities. Opening a remote
workspace folder through a controller-local filesystem path is unavailable.

## UI state vs backend data

| UI state (`State`) | Backend data |
|---|---|
| Tabs, active tab; per browser tab the address and history, per terminal tab the folder and find query; per project: shown file and change, open folders, Document/Source, scroll, accepted hunks (until applied), commit draft | Files, changes, hunks, checkpoints, documents, registered child transcripts |
| Visible transfer dialog and retained hook/settings drafts | Captured Node, worktree and resource owner for every read and save |

## Tests

`internal/desktop/live/workspace_projection_test.go` covers status and nested file references, unique hunk identity and commit scopes. `internal/desktop/panel/live_test.go` covers draft preservation across refresh and in-flight saves, exact file endings and native hunk selection.

`internal/desktop/panel/editor_test.go` groups exact source restoration, lossless
fallback selection, formatting boundaries, overflow ordering and close lifecycle
checks. Notebook draft validation, output projection and bounded embedded images
join the attachment projection group in `internal/desktop/live`.

`internal/desktop/live/panel_tabs_test.go`, a step of `TestPanelProjection`
against a real Node in Chinese: on an unsent draft and on a session every kind
of tab (Browser, Terminal, Files, Git, History, Documents, Artifact, Subagent,
a file and a diff) closes by its close button and by ⌘W, stays closed over
frames, the settings page and back, a Git refresh and a relaunch, the launcher
and "+" list History and Documents only with a session, and each body shows
its content or its empty state. `SAILRY_SNAPSHOTS` names a folder for the
launcher and empty-state frames.

`internal/desktop/live/browser_chrome_test.go`, a step of `TestPanelProjection`
against a real Node in Chinese and dark, with a typed-nil WebView placed by
`browserSurface`: the address policy, a new tab's empty state and focused
address, the loading state (bar, Stop, no selection), Stop and Reload, the
loaded state (the page's title in the strip), the page's frame equal to the
tab's body after panel resizes, window resizes, display scales and on the
frames of the panel's slide, the menu's items and their actions, a picked
element's validation, crop and context, its two attachments in the composer,
a stale pick's toast, and the failed state with its Reload. `SAILRY_SNAPSHOTS`
takes `browser-empty`, `browser-loading`, `browser-loaded`, `browser-menu` and
`browser-failed`; a native page never shows in a headless frame.
`TestLiveBrowserTabIntegration` drives the same chrome states and the menu in
English beside the Node's browser calls.

`internal/desktop/panel/model_test.go`: the live launcher's kinds per state, the
Documents tab without a document, the first change opening no diff, each empty
state and the loading spinner; tabs open once, closing picks the next,
items are flat tabs (new browser pages, retired kinds dropped), each file
and diff its own tab shared with the project (closing on either side,
another project swapping them), a browser tab's history (going drops the
forward part).
`internal/desktop/panel/git_test.go`: the grouped sections and the ⋯ menu's View
options regrouping them, a folder's right-click menu and its
confirmation, Commit's menu and amending, the history, All changes; the
file tree's click, ⌘-click, ⇧-click, ⇧↓, ⌘A, right-click selection and
Copy path, and its arrows, → and Enter.
`internal/desktop/shell/frame_test.go`: ⌘⌥B opens the launcher, ⌘P a file tab
beside the tree, a file chosen in the tree its own tab, ⌃⇧G and a change
a diff tab (read through the diff viewer's accessible text), choosing a
tab shows its item beside its list, eight tabs show the overflow menu,
the terminal has no Reset and ⌘F in it opens its find bar, a tab's close
button closes the shown tab and one behind it, ⌘W closes tabs
then the panel; collapsed, the header clears the traffic lights.
`internal/desktop/shell/pages_test.go`: the rail's Files and Git open pages, the
page's open files are the panel's file tabs, a change chosen in a tab
shows on the page, a diff closed on the page drops its tab, another
project keeps its own; the first frame of a page lays the side list out
at its width.

## Deviations

| Deviation | Reason |
|---|---|
| PDF and other non-native formats: WebView preview, later | MyGo has no native view of them; the card's Open uses the system's app meanwhile |
| Unsupported native rich-text structures edit in Source with Markdown preview | The pinned rich-text codec omits their structure; retaining source prevents data loss |
| The column's strip is in the header band (user decision 2026-11-11) | The reference draws it as the column's first row; docs/shell.md |
| Every open item is a tab in one flat strip | The plan asks for tabs; the reference holds one resource and tabs only a resource's documents |
| Checkpoints and documents are tabs | The reference restores checkpoints from a turn and draws documents in a plugin |
| History and Documents are not offered on an unsent draft | They show a session's turns and documents; a draft has neither |
| Files and Git keep their state per project | The reference keeps one and resets it when the project changes |
| Each open file and diff is a tab, also on the pages' strips | The plan asks for several files and diffs open at once |
| The split's list is a fixed column past our own drag handle | MyGo's `Split` sizes its first pane, so a first frame jumped |
| Hunks show in the Git tab's main area, not a dialog | One place for the preview's diffs |
| Preview Git forms, Output, commit and stash documents show the notice | Live adapters require a real repository; B0 remains isolated |
| Preview browser and terminal are placeholders in their chrome | The preview loads no page and starts no process; live tabs render (MyGo WebView, libghostty-vt) |
| Browser tabs belong to the conversation they were opened in and show in its panel only | The strip is the conversation's ([Tabs per conversation](#tabs-per-conversation)); the agent drives only its own conversation's tabs. A tab opened in a draft moves to the session that draft becomes, by the draft's exact ID when the session is created (`browserAdopt`); tabs of other drafts and tabs without a conversation are never taken over |
| Saving a browser capture as a session file is unavailable | The Node has no generated-media publication owner yet; inline captures are returned |
| A page that changes its own address (`history.pushState`) may leave the address field behind | The field follows the WebView's navigation events; unverified for same-document changes |
| Browser tools are listed without a connected browser | The turn catalog is frozen before a controller subscribes; a call then reports the disconnected controller |
| The terminal's find field reads MyGo's "Find", not `terminal_search_placeholder` | MyGo's `FindBar` does not expose its field; the toolbar's Find action carries the key |
| The in-memory browser store empties when its last tab closes | MyGo's ephemeral session ends with its last page; the reference keeps it until the app quits |
| The browser's chrome is core | In the reference the browser plugin draws it; the preview has no plugin UI host |
| The browser toolbar has a More actions menu and no Open or Settings button | The reference has a Go arrow and a gear and reaches the web inspector by ⌘⇧I only; Enter goes, and the menu holds the inspector and the settings |
| Loading shows a bar under the toolbar | The reference shows only the tab's spinner; the bar is indeterminate because pinned MyGo reports no load fraction |
| A failed page shows its error in the body, without a toast | The reference toasts `browser_load_failed` over the page WebKit leaves; an outcome is not shown twice |
| A picked element is two attachments, without an inline chip or a note | The composer has no reference kind for an element with a hover card; Alt-click, the reference's annotated pick, picks and ends the selection |
| A browser tab has no favicon | Pinned MyGo reports no favicon; the reference shows the globe as well |
| Controller-native folder opening is unavailable for remote workspaces | A remote path grants no controller filesystem authority and there is no supported remote folder-open effect |

## Native workspace activity

`workspace_activity.go` fills the existing composer Git capsule from the same
captured `InspectGit` state as the panel. Totals include every returned entry;
the recent list uses modification time descending, stable path order for ties,
and the existing ten-file limit. Its action opens the actual worktree diff.
Final turn diffs populate conversation changes cards. Review captures that turn's
patches and hunk identities in the existing Git surface; Undo confirms the target
and restores matching checkpoints in source order. A partially confirmed restore
resumes after its successful entries, whose files no longer match their
checkpoints, and retains uncertain request identity.

Project-backed unsent drafts resolve existing worktrees without creating a
session or invoking a model. Composer location fields keep paths as identities;
additive activity callbacks supply branch labels. Sidebar worktree controls use
`ListWorktrees`, register native worktrees before selecting them, and reuse the
existing Git forms for creation and removal. Confirmed worktree responses bridge
snapshot delivery without treating a display label as a path or identifier.

The native File menu's Save command follows the focused live file editor and
uses the same revision-checked save action as its toolbar and shortcut. Moving
focus outside an editable file disables the menu item; a clean editor keeps the
command available and saving it has no effect. File references in the composer
reuse this workspace's directory owner and receive its watched changes.
