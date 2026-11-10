# Shell

B8 connects Market and the installation rail to the live Client adapters.
`Hooks.PackageDialogs` renders captured package-discovery and AI CLI dialogs
beside the existing download dialog hook; it changes no rail or page layout.
The project overview and sidebar share one CLI launcher that reads available
tools from the project's owning Node and retains its chosen worktree through
launch. The authorized terminal prerequisite now displays that Node-owned AI CLI
through MyGo Mirror/Conn in the panel, with actual input, resize and close.
General terminal resources, split shell actions and browser surfaces remain
assigned to B10.

The main window's frame: the header band, the feature rail, the sidebar
column and the right panel beside the page, their motion and resizing,
the window's shortcuts and native app menu, the search, the activity
centre, the knowledge viewer and the UI preferences kept across launches
(the reference implementation `shell.rs`, `shell/{frame,frame/*,rail,rail/*,header,
splits,limits,search,search/*,activity,activity/*,knowledge,knowledge/*,
shortcuts}.rs`, `shortcuts.rs`, `app_menu.rs`, `activity/*.rs`).

## Package and files

`internal/desktop/shell`

| File | Holds |
|---|---|
| `shell.go` | `App`, `View`, the frame: columns, `Motion`, resize handles, the page and its actions |
| `header.go` | The header: brand, sidebar toggle, breadcrumbs, activity strip, preview tag, details toggle |
| `rail.go` | `Features`, `MoveFeature`, the rail, More, the host picker |
| `pages.go` | The rail features' pages, `Navigate`, `ClosePage`, the selected project's `panel.Project` |
| `project.go` | The captured project's overview, Git initialization confirmation and searchable session records |
| `shortcuts.go` | `Shortcuts`, `CloseFocused`, `KeyText`, the window's key handling, the menu's commands (`Do`) |
| `menu.go` | `Menu`, `Accelerator`: the native app menu |
| `search.go` | `Choice`, `Group`, `Filter`, `Next`, the search overlay |
| `activity.go` | `NoticeAge`, `SortNotices`, the activity centre and the header's strip |
| `knowledge.go` | `Source`, `ParseSource`, the knowledge viewer |
| `prefs.go` | `Prefs`, `LoadPrefs`, `SavePrefs`, `ClampNav`, `PanelFits`, `ClampPanel` |
| `hosts.go` | The host page's data and state, `openHost`, `addHost` (docs/host.md) |
| `transfers.go` | The Save as dialog's playback (docs/transfers.md) |
| `welcome.go` | The welcome guide over the window (docs/onboarding.md) |
| `selection.go` | `SystemShortcuts`, the selected text dialog |

## Entry point

```go
app := shell.New(preview.Load())
app.SetPrefs(shell.LoadPrefs(userData)) // at start
shell.SavePrefs(userData, app.Prefs())  // on blur, close and quit (cmd/sailry-desktop)
mygo.NewWindow(mygo.WindowOptions{StateKey: "main", Content: ui.View(app.View), ...})
```

`cmd/sailry-desktop` also imports `github.com/egoist/mygo/locales/zh-Hans`, so
MyGo's own strings follow the app into Chinese.

## Frame

| Metric | Value (reference) |
|---|---|
| Header | 48 (`HEADER_HEIGHT`); brand padded 80 for the traffic lights |
| Rail | 56; 36 DIP buttons, 8 apart |
| Sidebar | 240, 200..320 (`NAV_WIDTH`, `WIDTH_RANGE`); double click on its edge resets |
| Right panel | 560, at least 360, leaving the conversation 320 (`resources::MIN_WIDTH`, `limits.rs`) |
| Corner | `radius_lg` at the top left of the panel area, 1 DIP top and left outline in `sidebar_border` (`frame/corner.rs`) |
| Motion | 180 ms, cubic-bezier(0.2, 0, 0, 1) (`duration_normal`, `easing_move`) |

Each column animates one openness from 0 to 1 (`Element.AnimateTo` on the
frame). The column is its full width clipped to `width × openness`, so its
content slides rather than reflows: the sidebar from the left, the panel
anchored at the right edge. Content stays built until the column is
closed, and the resize handle shows only when it is fully open. With
Reduce Motion MyGo moves the columns at once. Whether the panel fits is
judged where the sidebar will end, and a narrow window hides it without
changing the choice; the details button then dims. The resize handles are
6 DIP strips over the edges (custom: `ui.Split` keeps both panes 40 DIPs
wide and cannot close one to nothing, which the motion needs).

## Rail

Pinned features in their order, then More; the bell, the host picker,
Search and Settings at the bottom, the installations button
(docs/transfers.md) after the bell while there are requests. Conversation is fixed first and Market
last. More is a popover to the right listing every feature with
a pin (`feature_pin`/`feature_unpin`); rows drag to reorder
(`MoveFeature`, a 4 DIP primary line). Every feature opens an
independent page in the main column, as the reference's `Page`
destinations do: Files and Git over the selected conversation's project
(the same `panel.Project` the panel's tabs show, `docs/panel.md`),
Activity and Market as the reference's preview shows them (`unavailable`).
The panel shows on the conversation page only; selecting a conversation
over a page goes back to it, and `ClosePage` closes the page shown. Hosts: a menu with the hosts (check on the chosen one, `App.Host`),
each opening its page (`HostPage`, docs/host.md, whose Update opens the
update dialog; no trailing Update in the menu), `host_add` opening
Settings › Connections' Add host; a dot when one has an update; its right-click menu
is the system's (`workspace_host` opens the chosen host's page,
`connection_action_edit`, `project_add`). The host page has the right
column too: its metrics, toggled by the details button and ⌥⌘B.

## Header

The sidebar toggle (`PanelLeftClose`/`Open`), then the title: on a
conversation in a project, breadcrumbs (`ui.Breadcrumbs`) of the project
and the conversation, the project revealing its rows in the sidebar; on
Files and Git, the project and the page; on Activity and Market, its
name. On
the right the activity strip, the preview tag and the details toggle
(`PanelRight`, selected while the panel shows). Tooltips show the
shortcut.

## Shortcuts

Keys go to the innermost scope that takes them, as MyGo dispatches a key,
and a key one scope takes never reaches the next:

1. Floating layer: the focus inside a dialog, popover or the search, then
   `Element.OverlayShortcut` of the overlays, the top one first
   (`PopoverBase` and `DialogBase` close on Escape).
2. Focused pane: the right panel and the main column (a page) take the
   focus as they are clicked (`Focusable`, a `ui.Handle`) and ⌘W through
   `Element.Shortcut` while it is inside them (`App.pane`).
3. Conversation: the composer's text input takes its editing keys first.
4. Window: `Context.Shortcut` (`App.shortcuts`), which a modal dialog
   holds.
5. App: the native menu (`menu.go`).

| Key (macOS) | Scope | Action |
|---|---|---|
| Esc | floating layer | Close the topmost layer only (the search clears its query first) |
| ⌘W | right panel focused | Close its active tab, the panel with its last; the focus goes back to the conversation |
| ⌘W | Files or Git page (any independent page) focused | Close the page, back to the conversation |
| ⌘W | anything else | Nothing |
| ⌘A ⌘C ⌘V ⌘X ⌘Z ⇧⌘Z, arrows, ⌘← ⌘→ | text input | Edit the text (Edit menu roles) |
| ⌘N | window | New conversation |
| ⌥⌘N | window | Project editor |
| ⌘O, ⇧⌘R | window | Open folder, Sync: preview toast |
| ⌘K | window | Search |
| ⌘B | window | Sidebar |
| ⌥⌘B | window | Right panel |
| ⇧⌘[ / ⇧⌘] | window | Previous / next conversation (`sidebar.Adjacent` over `Order`) |
| ⌘/ , ⌘, | window | Settings |
| ⌘T, ⌃`, ⌘P, ⌃⇧G | window, conversation page | Browser, Terminal, Files, Git in the panel |
| ⇧⌘W | app | Close Window |
| ⌘Q | app | Quit |
| ⌘H, ⌥⌘H, ⌘M, ⌃⌘F | app | Hide, Hide Others, Minimize, Full Screen |
| ⌃⌥Space | system | Bring the window forward with the search (`system.activate`) |
| (none by default) | system | Selected text (`system.selection`) |

The system's shortcuts are `App.SystemShortcuts`, which `cmd/sailry-desktop`
registers with MyGo's `GlobalShortcut` at launch (system_shortcuts.rs
install); each brings the window forward and runs its command through
`Do`. The selection shortcut's dialog (system_shortcuts.rs show) shows
the captured text, selectable, Read aloud (preview_notice) and Quote, which
adds it to the draft of the conversation shown as it was captured. The
preview reads no other app: it shows `preview.Selection`.

⌘ is Ctrl off macOS (`ui.Cmd`); the app menu's Close Window, Hide, Hide
Others, Minimize and Full Screen are on Command (`ui.Super`) as
`app_menu.rs` binds them. `KeyText` writes keys for tooltips and the
launcher, `Accelerator` for the menu.

## App menu

`App.Menu(lang, send)` returns `app_menu.rs`'s menus for
`mygo.App.SetMenu`, labelled by the `menu_*` keys: App (About, Settings,
Services, Hide, Hide Others, Show All, Quit), File (New Conversation, New
Project, Open Folder, Save, Close Tab, Close Window), Edit (Undo, Redo,
Cut, Copy, Paste, Select All), View (Search, Find in Conversation, Toggle
Sidebar, Toggle Details, Full Screen, then the rail's features), Window
(Minimize, Zoom, Bring All to Front) and Help (Website, Feedback, Logs,
then the preview's Welcome to Sailry and Selected text).
The system's items are MyGo roles. The app's items send their command's
ID, which `cmd/sailry-desktop` passes to `App.Do`.

The menu takes its items' keys before the window on every platform, so a
command from the menu runs in the next frame scoped as its key would be:
Close Tab closes what ⌘W closes where the focus is (`closeFocused`, by
`Context.FocusWithin` of the panes' handles), and the window's commands
wait while the search, the knowledge viewer or the project editor shows,
as a modal dialog holds the window's shortcuts. This is the only scoping
added over MyGo's own.

## Search

⌘K or the rail: a dialog 480 wide. Root rows: Chats,
Projects, Hosts, Settings (folders) and the features. A folder lists its
items with a check on the current one. `Filter` keeps rows whose label or
keyword holds the query, case folded, in order, dropping empty groups;
`Next` moves round. Enter opens a folder or runs a row, Escape clears the
query then closes, Backspace on an empty query goes back, `reference_back`
too. Empty: `connection_search_empty`.

## Activity centre

The bell, a red dot while notices are unread, opens a popover to
the right: `notification_center` (preview toast), Mark all as read,
Delete read, then the notices newest first (`SortNotices`): a 40 DIP row
with the kind's glyph and color (completed, failed, approval, input,
info), the title (medium while unread), `NoticeAge`, the unread dot. A
click reads it and opens its conversation, or its knowledge source.

The header's strip (shell/activity/strip.rs) shows waiting, running and
finished-unread sessions, chats included, in groups 12 DIPs apart: 28 DIP
text buttons 4 apart, each a 20 DIP agent mark (`kit.Agent`, the
reference's eight SVG designs and colors). A running one turns in its
color; the others take the lane's color with a 10 DIP badge (warning
triangle, success check). A group shows three (two under 480 DIPs), or two
and +N in the lane's color, which opens a 300 DIP popover of the group's
rows. The pointer resting on one shows its title and project in a
tooltip.

## Knowledge

`ParseSource` accepts `sailry-knowledge://<collection>/<chunk>?revision=<r>`
only, as the reference. The viewer is a dialog up to 800 wide:
`knowledge_source`, the title, `host · path · Lines a–b`, the page in mono,
Previous, Next and Close. The preview pages its citation in memory; the
activity centre's knowledge notice opens it.

## UI preferences

`Prefs` keeps the sidebar's width and openness, the panel's width,
openness, tabs and active tab, the projects closed in the sidebar, and
the rail's order and unpinned features, in `ui-state.json` in MyGo's user
data folder (beside `window-state.json`, which `StateKey` keeps the
bounds in). `LoadPrefs` falls back to the defaults and clamps widths.

## Backend ownership

The live adapter and controller preferences are specified in
[B3 live wiring](#b3-live-wiring) and
[B7 remote wiring](#b7-remote-wiring). The preview remains an explicit, separate
constructor and does not own backend facts. Feature phases supply their owning
Client actions and projections without changing the accepted UI.

## UI state vs backend data

| UI state (`App`) | Backend data |
|---|---|
| Page, column widths and openness, rail order and pins, the search's folder, query and row, popovers, the viewer's page | Hosts, notices and their read state, conversations, knowledge pages |
| Chosen Host and pending session selection in a new window | Each execution Node's separate Client snapshot, verified peer metadata and connection state |

## Tests

`internal/desktop/shell/shortcuts_test.go`: ⌘W in a focused panel tab, a focused
Files or Git page and with nothing closable focused; Escape closing only
the popover; a text input keeping ⌘A and ⌘Z; the menu's items, keys and
translations; menu commands scoped as their keys.
`internal/desktop/shell/frame_test.go`: ⌘⌥B, ⌘P, ⌃⇧G and ⌘W on the panel;
⌘B and preferences round-tripping; ⌘K search opening a conversation;
the activity centre opening a conversation and the knowledge viewer
paging; `ParseSource`, `Filter`, `Next`, `MoveFeature`.
`internal/desktop/shell/pages_test.go`: the rail opens pages, not tabs; the
Files and Git page and tab share a project's state, another project
keeps its own.
`internal/desktop/shell/sidebar_test.go` and `shell_test.go`: the sidebar and
Phase 1–2 screens.

## Deviations

| Deviation | Reason |
|---|---|
| Breadcrumbs in the header | The plan asks for them; the reference shows a title |
| The resize handles are drawn strips | `ui.Split` cannot close a pane to nothing |
| Preferences are a JSON file beside `window-state.json` | MyGo's `WindowState` has no fields for the app's own state |
| The notification center page, Activity board and Market | Later phases; Activity and Market show `unavailable`, Market not yet in Settings |
| ⌘/ opens Settings | No Shortcuts section before phase 5 |
| The rail's More rows are a popover, not a menu | Rows hold a pin button and drag to reorder |
| System shortcuts register at launch only | Keys changed in Settings take effect at the next launch |
| The selected text is the preview's sample | Reading another app's selection needs the platform's accessibility APIs and a Node |
| About opens Settings, which has no About section | Settings sections beyond Appearance, Providers and Plugins are later phases |
| Save (⌘S) and Logs are disabled | The preview has no file editor or log folder (the reference disables Logs without one) |
| Help lists Welcome to Sailry and Selected text | The preview's way to the first launch's guide and the system shortcut's dialog |
| ⌘., ⌘L, ⇧⌘M, ⇧Tab and the conversation's Escape targets are not bound | The live conversation's controls (`conversation/live/shortcuts.rs`) come with the live session |
| The View menu's features are those at launch | The menu is set once; rail reordering does not rebuild it |

## B3 live wiring

`live.NewWithPreferences` composes the accepted shell over the already started
shared Client. `viewmodel.Data` and Workspace DTOs are neutral presentation types;
preview aliases retain the explicit preview constructor and its interactions.
`App.Hooks` injects refresh, folder selection and notice operations. Background
callbacks only enqueue work. `App.View` drains it on the UI thread; `SetUpdate`
binds MyGo Window.Update to schedule the frame. The live path never constructs
fixtures, advances preview streams or simulates transfers, dictation or host
metrics. Models, tools, file/Git data and later-phase settings stay unavailable.

Search, breadcrumbs, the local host overview, activity lanes and pane existence
follow Client snapshots. Sync calls Client.Refresh, replacing the Node topic
subscription rather than replaying a command. Open directory uses MyGo's system
file URL opener. The window factory receives the selected session and shares the
existing Node/Client. New Conversation creates only a local empty draft; B4 owns
first-send session persistence and model transport.

The bell reads Client's durable and attention inbox. Its centre opens a separate
notification page with All/Unread filters, expandable typed details and a copyable
technical report that excludes authored title/body and raw provider errors.
Copying the report emits `notification_report_copied` after the clipboard write.
Read/dismiss use captured identifiers and ordinary Node commands; transient
attention read/dismiss remains Client-owned. One application-lifetime Notifier
observes Client independently of frames, suppresses categories from controller
preferences and delivers each new notice once. Its UI scheduler remains alive
when all windows are closed; it closes before Client shutdown.

Profile preferences are one shared Store across windows. Each window synchronizes
its revision before rendering; disk writes are serialized immutable snapshots,
and a stale captured version cannot overwrite a newer edit. Welcome state is in
`desktop/preferences.json`. `ui-state.json` still restores column widths,
collapsed projects, rail state, tabs and pane layout; its legacy welcome field is
used only by the preview. Closing a live guide persists the profile choice.

The new notification centre uses existing Kit page/card/segmented controls. Live
host views hide preview-only notes; B7 supplies their real facts and metrics.
Terminal panes retain the accepted static preview until their resource phase
and never start a process.

## B4 conversation wiring

`Hooks.Conversation` supplies live transcript and composer data/actions for each
visible session, including split panes. Runtime drains Client callbacks only on
the window UI thread. Observers are shared by Client, retained while visible and
released after a frame hides their pane. New drafts stay local until first send;
a confirmed session result migrates the draft and pane identity before admission.
Models and conversation work now use the B4 adapters; later file, terminal and
plugin services retain their phase boundaries.

The sidebar's whole-session export and Fork here use the application export
callback and `ForkConversationAt` respectively, with captured session/worktree
ownership. Reply menus use the same export callback. The application reader
injects its global active predicate, so speech completion and another window's
speech update Read/Stop controls. Native selection capture calls `ShowSelection`;
its Read action uses a distinct selection/session key on the same reader service.
Preview keeps its local interactions and never reaches these live hooks.

## B7 remote wiring

Application startup creates one `live.Network` after the local Client starts.
All windows share its remembered peers and remote Clients. Discovery responds
to peer changes and the source's ten-second observation cadence; it verifies
execution capability and protocol compatibility before requesting a Node
snapshot. Initially offline peers remain remembered and can appear without
pairing again. Paired Mobile controllers stay in the device list and do not
receive Node commands.

Each window creates a bound Runtime for each execution Node. Its combined
presentation adds Node identity to projects, conversations and notices while
retaining separate authoritative snapshots. Rail and search Host choices,
header names and the Host page consume verified Host facts. The update dot
compares the advertised version with the local Host build by SemVer precedence.
It does not require opening the update dialog first. Edit network targets the
captured Host; Add Host opens the existing Connections flow.

Conversation and workspace actions resolve their execution Runtime before
asynchronous work starts. A new window retains the requested remote session
until that owner's snapshot arrives. Notice read/dismiss actions route through
the owning Client. Sync and conversation Retry refresh the selected execution
owner rather than changing local Node authority.

The project overview is an independent `ProjectPage`, opened through
`App.OpenProject` with captured presentation actions. Its `ProjectOwner` accessor
retains the exact Node and project while the page is shown, including after a
projection disappears; it never substitutes a same-ID project on another Node.
The header names that project. Its 800 DIP content column uses the existing Kit
controls for project facts, New session, Git initialization and session records.
Records keep archived sessions and search by title or worktree path. Live pages
hide the workspace preview note. The explicit preview retains its existing
project-menu behavior. Terminal resources remain B10 and AI CLI remains B8.
Choosing a project worktree opens the same page with that worktree captured by
its Git and New session actions; ordinary Overview starts on the main worktree.

Window shutdown releases that window's observations, transfer dialogs and
Forwarders. It does not close shared Clients or stop admitted work. Application
shutdown closes window Runtimes, then Network, the local Client and Node.
Preview never constructs Network or opens a profile.
