# Shell

B8 connects Market and the installation rail to the live Client adapters.
`Hooks.PackageDialogs` renders captured package-discovery and AI CLI dialogs
beside the existing download dialog hook; it changes no rail or page layout.
The project overview and sidebar offer no AI CLI entry
([removed UI](removed-ui.md), rows 22 and 24); the launcher dialog that reads
the owning Node's tools stays in `live` without an entry. The terminal
prerequisite displays a Node-owned terminal through MyGo Mirror/Conn in the
panel, with actual input, resize and close.
General terminal resources, split shell actions and browser surfaces remain
assigned to B10.

B9 adds metadata-driven plugin rail entries, More items, native menu items and
search rows. A selected entry mounts its immutable desktop entry on the captured
Node and surface; worktree entries keep the selected worktree. Removing an
active entry returns to the conversation. The activity board reads real Node
catalogs and previews through Client. Optional packages keep their installed and
enabled choices; this wiring adds no bundled defaults.

Plugin headers use the active navigation or resource mount. Header titles,
filters, actions and tabs retain that mount's generation. Composer header
contributions use the exact session or unsent-draft Registry. A disconnected
mount keeps its last presentation with actions disabled until reconnection
creates a fresh owner.

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
| `project.go` | The captured project's overview, Git initialization confirmation and its searchable records: Sessions and Terminals in the list Settings pages use (`kit.List`: the filled card, rows at the row metrics a hairline apart, the empty state inside the card), each row a button with the neutral hover fill across the card, a session's with its right-click menu |
| `shortcuts.go` | `Shortcuts`, `CloseFocused`, `KeyText`, the window's key handling, the menu's commands (`Do`) |
| `menu.go` | `Menu`, `Accelerator`: the native app menu |
| `search.go` | `Choice`, `Group`, `Filter`, `Next`, the search overlay |
| `activity.go` | `NoticeAge`, `SortNotices`, the activity centre and the header's strip |
| `knowledge.go` | `Source`, `ParseSource`, the knowledge viewer |
| `prefs.go` | `Prefs`, `LoadPrefs`, `SavePrefs`, `ClampNav`, `PanelFits`, `ClampPanel` |
| `panels.go` | `ConversationPanel`, `MountPanel`: the right panel per conversation (docs/panel.md, Tabs per conversation) |
| `hosts.go` | The host page's data and state, `openHost`, `addHost` (docs/host.md) |
| `transfers.go` | The Save as dialog's playback (docs/transfers.md) |
| `welcome.go` | The welcome guide over the window (docs/onboarding.md) |
| `selection.go` | `SystemShortcuts`, the selected text dialog |
| `closing.go` | `CloseRequest`, `ConfirmClose`, `Closing`: the close and quit prompt |
| `closing.go` | `CloseRequest`, `ConfirmClose`, `Closing`: the close and quit prompt |
| `demo.go` | Demo mode: the window's two stages, `Demo`, `DemoConversation`, `DemoReveal`, `DemoUsage`, `Prefs` |

## Entry point

```go
app := shell.New(preview.Load())
app.SetPrefs(shell.LoadPrefs(userData)) // at start
shell.SavePrefs(userData, app.Prefs())  // on blur, close and quit (cmd/sailry-desktop)
mygo.NewWindow(mygo.WindowOptions{StateKey: "main", Content: ui.View(app.View), ...})
```

`cmd/sailry-desktop` also imports `github.com/egoist/mygo/locales/zh-Hans`, so
MyGo's own strings follow the app into Chinese.

### Start failures and a second launch

A Desktop that cannot start its Node (`cmd/sailry-desktop/failure.go`) runs the
MyGo application only to show one native error message
(`mygo.Dialog.Message`, type `error`) and quits: `start_failed` with the
failure's own text as the detail, or `start_profile_in_use` with the profile
path when another process holds the profile lock. The failure is still written
to standard error.

A Desktop on the default profile takes MyGo's single-instance lock
(`App.RequestSingleInstanceLock`) before it opens anything. A second launch
hands over to the first and exits; the first raises its active window, or
opens one when none is left (`App.OnSecondInstance`). A Desktop started with
`--data-dir` or `--preview` takes no lock, so development profiles run beside
the default one; a second Desktop on the same explicit profile gets the
message. The profile lock itself (`flock`, `LockFileEx`) ends with the process
that holds it, so a crash leaves no stale lock; the lock file stays in place
and never blocks a start.

The updater starts the new application before the old one has quit. That
launch would be handed to the Desktop that is quitting and exit with it, so
`desktopBackend.Stop`, which the updater calls before it replaces or restarts
the application, marks the environment the new process inherits
(`SAILRY_DESKTOP_SUCCESSOR`). A successor asks for the lock every 100 ms, for
at most ten seconds, until the Desktop it replaces has quit (`instanceLock`),
and that Desktop ignores the hand-overs meanwhile.

| Deviation from Rust | Reason |
|---|---|
| Start failures are shown in a native message | Rust prints them to standard error, where a user who opened the application from its icon never sees them |
| A second launch raises the running Desktop | Rust starts a second process that fails on the profile lock |

Not covered: a launch while the previous Desktop is still inside its quit
sequence is handed to that process, which is closing and shows nothing. The
Node's shutdown deadline bounds that window at five seconds.

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
changing the choice; the details button is then disabled, since it cannot
act (⌥⌘B still changes the choice). The resize handles are
6 DIP strips over the edges (custom: `ui.Split` keeps both panes 40 DIPs
wide and cannot close one to nothing, which the motion needs).

A conversation pane (the main column, or a pane of a split) is the
transcript filling it from under the header to its bottom, with the
composer floating over the transcript's end (`conversation.Docked`, see
`docs/conversation.md`), then the navigation rail over both. The welcome
page hosts its composer itself, and the child transcript of the right panel
has no dock.

### Side column per page

The column beside the rail belongs to the page (`App.HasNavigation`, the
reference's `feature_navigation` in `shell/rail.rs`):

| Page | Side column |
|---|---|
| Conversation, Host, Project | The session sidebar (`sidebar.View`) |
| Settings | The settings sections (`settings.Nav`) |
| Files, Git, Activity, Market, Notification center, a plugin's page | None; the page starts at the rail and lays out its own columns |

`NavOpen` and `NavWidth` are one saved choice shared by the pages that
have a column, as the reference's `layout.sidebar_open` and
`sidebar_width`; `App.NavShown` is the choice on a page that has one. A
page without a column hides it without changing the choice: its header
has no sidebar toggle and no brand text, and ⌘B and the menu's Toggle
Sidebar do nothing there (`toggleNav`, the reference's `toggle_sidebar`).
Going to or from such a page shows the destination settled, with no
column motion. The right column's openness and width are the
conversation's and the host page's as before.

## Rail

Pinned features in their order, then More; the bell, the host picker,
Search and Settings at the bottom, the installations button
(docs/transfers.md) after the bell while there are requests. Conversation is fixed first and Market
last. More is a popover to the right listing every feature with
a pin (`feature_pin`/`feature_unpin`); rows drag to reorder
(`MoveFeature`, a 4 DIP primary line). Every feature opens an
independent page in the main column, as the reference's `Page`
destinations do: Files and Git over the selected conversation's project
(the same `panel.Project` the panel's tabs show, `docs/panel.md`; in a
live window `Hooks.WorkspaceProject`, the selected session's worktree read
from its Node), `unavailable` while a project's worktree is not known, and
without a project `resource_project_empty` over New project (`project_new`),
which opens the project editor (`App.projectPrompt`; the page has no sidebar,
so it builds `sidebar.Dialogs`),
Activity and Market as the reference's preview shows them (`unavailable`).
The panel shows on the conversation page only; selecting a conversation
over a page goes back to it, and `ClosePage` closes the page shown. Hosts: a menu with the hosts (check on the chosen one, `App.Host`),
each opening its page (`HostPage`, docs/host.md, whose Update opens the
update dialog; no trailing Update in the menu), `host_add` opening
Settings › Connections' Add host; a dot when one has an update; its right-click menu
is the system's (`workspace_host` opens the chosen host's page,
`connection_action_edit`, `project_add`; then `App.hostMenu`: `port_mappings`,
a submenu of the chosen host's forwards, each opening its address, over
`port_title`, which opens the host page's port picker; `sidebar_host_update`
with the name of every host that has an update, opening its page with the
update dialog, since the approved picker rows carry no trailing Update; and,
for a paired host, `live_revoke`, which opens the page's confirmation). The host page has the right
column too: its metrics summary, closed until the details button or ⌥⌘B
opens it.

A host's name shows as it is (`App.hostName`): only the preview's hosts and
the local host before its Node names it (`local_host_name`) are i18n keys, so
a device named like a catalog key is not translated.

## Header

The sidebar toggle (`PanelLeftClose`/`Open`) on a page with a side
column, then the title: on a
conversation in a project, breadcrumbs (`ui.Breadcrumbs`) of the project
and the conversation, the project revealing its rows in the sidebar; on
Files and Git, the project and the page, the project going back to the
conversation with its rows revealed; on Activity and Market, its
name. The main region ends, on the conversation page, with the activity
strip (session status), then the conversation's own actions
(`conversationActions`): Search (`chat_search`, the find bar ⌘F opens) and
the session overview (`composer.OverviewButton`, `Hooks.Overview`; see
`docs/composer.md`), then
the plugins' header contributions (`Hooks.PluginHeader`) and the preview tag. A plugin's header
contribution is the same `kit.IconLook` at `HeaderButton` as the shell's own
buttons, `ButtonGap` apart, its popover under its end; an `indicator` or
`metric` is not drawn in the header. Tooltips show the shortcut. The window
opens and closes a header `popover` itself, on the press, an outside press
and Escape, as the source does: the package is not asked, so a package that
answers no `change` event still opens. The popover is `kit.Popover` with the
package's panel (its `handler`) in a scroll: the declared width, 480 without
one, at most the window less 32, and at most the window's height less 110.

The band is two regions, split at the right column's left edge
(`headerSide`):

| Region | Content |
|---|---|
| Main, any width | Sidebar toggle, title, then at its right end, left of the split: the activity strip (session status), Search and the session overview, the plugins' contributions and the preview tag |
| Side, as wide as the column shows (`panelW × details`), at least the details toggle (`sideWidth`) | The column's segment while the column shows, then the details toggle alone |

- The details toggle (`PanelRight`, selected while the column shows) is the
  last control of the window in both states, `HeaderInsets` from its edge:
  12 plus `c.TitleBar().Right`. Nothing but the system's caption buttons is
  right of it, and nothing but the column's segment is left of it in the
  side region.
- The activity strip is the main region's (user decision 2026-11-11,
  replacing the same day's placement beside the toggle): directly left of
  Search and the overview, `ButtonGap` apart, with the column open or
  closed. Where the conversation has no such actions (no turn yet, or a
  split) it ends the region before the plugins' contributions and the tag.
  Without a group it takes no room and no gap.
- The title and the strip share one row (`lead`) that takes what the
  region's other controls leave. The title is at its start and gives way
  first, shortening with an ellipsis and cut at its own edge, so it is never
  under the strip. Once the title is down to `titleRoom` (96) the strip's
  groups show fewer sessions, down to each group's +N alone; where even
  that does not fit, the last groups leave (finished first, waiting last)
  rather than cover the actions.
- The strip must know its room in the frame it builds. The region's width
  comes from the frame's columns and the known controls from their
  constants; what the plugins' contributions and the tag take is read from
  the last frame's layout (`band.room`). Only a frame built on a measure
  that then changed asks for one more frame, so the header at rest asks for
  none.
- Search and the overview are one conversation's. A conversation shown
  alone has them in the band. In a split the band has none, and each
  conversation pane has its own in its header (`paneHeader`), 28 square and
  4 apart before the drag handle and the close button, acting on that
  pane's conversation; a terminal pane has none, and neither has a
  conversation without a turn. The search bar's state is kept by
  conversation (`conversation.State.finds`) and the overview popover's too
  (`App.overviews`, forgotten once its button is no longer built), so two
  panes can search at once and one pane's overview leaves another's as it
  is. ⌘F and the menu's Find act on the focused pane's conversation
  (`findTarget`), and on none while a terminal pane has the focus.
- While the column shows, a 1 DIP line in `Border` continues the column's
  left border through the band. Both are placed from the same
  `panelW × details`, so they line up while the column opens, closes or is
  resized.
- The segment belongs to the column's content (`panelHeader`). On a
  conversation it holds the panel's tab strip (`panel.Strip`: tabs, the
  overflow chevron, "+"); there is no second strip row under the band, and a
  tab's own toolbar stays in the body. Under extension details (Settings and
  Market: plugin, skill, theme) it shows `settings.DetailsTitle`, and on the
  Host page `metrics_title`, 12 from the split; those columns have no title
  row and no close button of their own.
- The toggle is the column's one close control. It closes the extension
  details (`Settings.CloseDetails`), the metrics and the panel. It is
  disabled when the page has nothing to show there (`hasDetails`: Market and
  Settings with no card selected, Files, Git, Activity, Notifications and
  plugin pages) and when the window has no room for the column. Selecting a
  card opens the details.
- The segment and the side row are not interactive, so the band's empty
  parts, the gap between the tabs and "+" included, drag the window.
- ⌘W with the focus on a tab of the band closes the active tab, as it does
  with the focus in the column.

`c.TitleBar()` in pinned MyGo: macOS reports the traffic lights as `Left`
and no `Right`; Windows reports 46 for each caption button it shows as
`Right` (138 for three) at 32 high; Linux reports the measured header-bar
button widths as `Left` and `Right` by the desktop's button layout, and
nothing where the compositor decorates; every system reports zero in full
screen. The headless backend's `SetTitleBar` stands in for them in tests.

### Icon buttons

Every icon button of the header and the rail is `kit.IconButton`
(`kit.IconLook` on a menu's trigger), with Kit's ghost look: the theme's
radius, `Hover` under the pointer, `Active` while pressed or selected,
MyGo's dimming while disabled. No button draws its own. The installations
button (docs/transfers.md) wears the same `kit.GhostLook` at the rail's
size around its badge.

| Where | Size | Icon | Gap | Reference |
|---|---|---|---|---|
| Header (`HeaderButton`): sidebar toggle, details toggle, plugin contributions | 28 × 28, centred in the 48 band | 15 | 8, 12 from the window's edge or its caption buttons (`HeaderInsets`) | `Button::ghost().small()`: 24 × 24, icon 14, groups `gap_2` |
| Rail (`RailButton`): features, More, bell, installations, hosts, Search, Settings | 36 × 36 | 15 | 8, 8 under the band | ghost `size_9`: 36 × 36, icon 16, `gap_2` |
| Activity strip avatars | 28 × 28 | avatar | 4 in a group, 12 between groups | `.size_7()`, `gap_1`, `gap_3` |

A control that cannot act is disabled or absent, never a normal button
whose click does nothing: the details toggle without room for the panel
or without anything to show is disabled, and the sidebar toggle is absent without a side column. New
session over the unsent chat focuses its composer again (docs/sidebar.md).

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
| ⌘B | window | Sidebar, on a page with a side column |
| ⌥⌘B | window | Right panel |
| ⇧⌘[ / ⇧⌘] | window | Previous / next conversation (`sidebar.Adjacent` over `Order`) |
| ⌘/ , ⌘, | window | Settings |
| ⌘T, ⌃`, ⌘P, ⌃⇧G | window, conversation page | Browser, Terminal, Files, Git in the panel |
| ⌘. , ⌘L, ⇧⌘M | window, conversation page, focus outside the panel | Stop the running turn, focus the composer, open the model controls (the settings popover when compact). Fixed, as the reference binds them to the conversation: not in the Shortcuts settings or the menu, and no shortcut may be recorded on them (`settings.Fixed`) |
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
adds it to the draft of the conversation shown as it was captured, every
line of it quoted (`conversation.Quote`) after a blank line when the draft has
text, and nothing after the quote. The preview reads no other app: it
shows `preview.Selection`.

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
wait while the search, the knowledge viewer, the project editor or any
modal dialog shows, as a modal dialog holds the window's shortcuts: Kit's
dialogs note themselves on the window (`kit.NoteModal`) and `App.shortcuts`
reads the note once a frame (`kit.TakeModal`). This is the only scoping
added over MyGo's own.

An item's key is the one bound in Settings (`syncMenuKeys`): a rebound key
reaches the menu in the next frame and a cleared one leaves its item without
a key. Recording a shortcut holds the window's shortcuts and the menu's
commands only while the Shortcuts section shows; leaving it, however the
page was left, cancels the recording.

## Search

⌘K or the rail: a dialog 480 wide. Root rows: Chats,
Projects, Hosts, Settings (folders) and the features. A folder lists its
items with a check on the current one; a project opens its page
(`sidebar.Actions.Overview`, the sidebar's Project), and in the preview,
which has no such page, its rows in the sidebar. `Filter` keeps rows whose label or
keyword holds the query, case folded, in order, dropping empty groups;
`Next` moves round. Enter opens a folder or runs a row, Escape clears the
query then closes, Backspace on an empty query goes back, `reference_back`
too. Empty: `connection_search_empty`. Every way of opening it
(`openSearch`) starts at the root with an empty query.

## Activity centre

The bell, a red dot while notices are unread, opens a popover to
the right: `notification_center` (preview toast), Mark all as read,
Delete read, then the notices newest first (`SortNotices`): a 40 DIP row
with the kind's glyph and color (completed, failed, approval, input,
info), the title (medium while unread), `NoticeAge`, the unread dot. A
click reads it and opens its conversation, or its knowledge source.

### Notification centre

The centre's page (`notificationCenter`, `shell/notifications.go`) is the
All/Unread filter, Mark all as read and Delete read over one `kit.Card` of the
notices, newest first, apart by the card's inset separators; with none to show
it is the card-shaped `notifications_empty` result. Each notice is a
`kit.Bleed` row at the row metrics (`notificationRow`):

- the kind's glyph and color in the lead slot;
- the title, medium while unread, over the notice's message on one muted,
  ellipsized line (the user's decision of 2026-11-12), then its age
  (`noticeAge`, as the bell writes it) at the end of the row. A notice
  without a message has the kind's label (not for an info notice) and its
  host (`noticeSource`) on that line, and so has the open notice, whose
  message is whole in its details: it is written once;
- the unread dot, whose room stays when read, then the quiet 28 DIP icon
  buttons with tooltips: Open conversation (the chevron, only for a notice
  whose conversation is listed; its room stays too) and Delete.

The row's box takes the neutral hover fill from one edge of the card to the
other, as the project page's records do, whichever part the pointer is over.
The button that opens the notice's details runs from the card's edge to the
actions, which are its siblings: MyGo leaves a button built inside a button
out of the accessibility tree. A click on it opens the details under the row
and reads the notice; a second click closes
them. The details (`notificationDetails`) are indented to the title: message
and detail text, the facts as `kit.Record`s, and the report with its copy
button. Under Unread the open notice stays listed though opening it read it.

The header's strip (shell/activity/strip.rs) shows waiting, running and
finished-unread sessions, chats included, in groups 12 DIPs apart: 28 DIP
text buttons 4 apart, each a 20 DIP agent mark (`kit.Agent`, the
reference's eight SVG designs and colors). A running one turns in its
color; the others take the lane's color with a 10 DIP badge (warning
triangle, success check). A group shows three (two under 480 DIPs), or two
and +N in the lane's color, which opens a 300 DIP popover of the group's
rows. The pointer resting on one shows its title and project in a
tooltip. It sits in the header's main region, left of Search and the
overview, and folds to fewer sessions per group, then to each group's +N,
where the region is short (see Header).

## Knowledge

`ParseSource` accepts `sailry-knowledge://<collection>/<chunk>?revision=<r>`
only, as the reference. The viewer is a dialog up to 800 wide:
`knowledge_source`, the title, `host · path · Lines a–b`, the page in mono,
Previous, Next and Close. The preview pages its citation in memory; the
activity centre's knowledge notice opens it.

Live wiring (`internal/desktop/live/knowledge.go`, contract in
[knowledge.md](knowledge.md#desktop)): `Hooks.OpenKnowledge` receives the
parsed `Source` and the Node of the notice or conversation that named it. A
live notice names a citation when its `text` details are exactly one such
link (`knowledgeLink`, set by `Notices` in `live/projection.go`); its click
marks it read (`mark_notification_read`) and opens the viewer. A
link `ParseSource` rejects, or whose collection is not an identifier or whose
chunk or revision is not a 64-digit hash, toasts `knowledge_citation_invalid`
and sends nothing. Otherwise the controller reads `read_knowledge_citation`
(`context: false`, offset 0, limit 16 KiB) on that Node and
`App.ShowKnowledge` opens the viewer on the answer: its title,
`host · path · position` (`knowledge_lines`, `knowledge_page`,
`knowledge_cells`, `knowledge_slide`, a section's name or a value's key and
pointer) and text. The viewer is the last link's: the answer to an earlier
link that arrives after a newer one was opened is dropped. Next reads the page at `next_offset`, Previous the offset
before it; each is disabled at its end and while a page loads, and a failed
read keeps the page shown. Faults toast `knowledge_citation_changed`
(revision conflict, not found), `knowledge_denied`,
`knowledge_citation_invalid` or `knowledge_citation_unavailable`, the last
also for an answer that does not match its request. Replies open
`sailry-knowledge:` links through `conversation.State.OpenLink`; other links
open as before when `ui.SafeToOpen` allows them.

## UI preferences

`Prefs` keeps the sidebar's width and openness, the panel's width,
openness, tabs and active tab, the projects closed in the sidebar, the
preview's sidebar order (`sessionOrder`; a profile keeps its own in its
preferences, docs/sidebar.md) and
the rail's order and unpinned features, in `ui-state.json` in MyGo's user
data folder (beside `window-state.json`, which `StateKey` keeps the
bounds in). `LoadPrefs` falls back to the defaults and clamps widths.
The sidebar's openness and width are one choice for every page with a side
column; pages without one neither read nor change it, so the file's fields
are unchanged and earlier files load.
The file is one for the preview and every profile, so the rail's order and
pins in it are the preview's: a live window keeps its own in the profile
(`FeaturePreferences`), neither reads nor changes the file's, and writes
back the ones it loaded. The panel's tabs are per conversation: `panelTabs`,
`panelTab` and `panelOpen` are those of the conversation `panelOf` names, and
`panels` holds the other conversations'. A file without `panelOf` loads as
before, its tabs going to the first conversation shown
([panel.md](panel.md#tabs-per-conversation)). Artifact and subagent tabs are not saved: their
content does not outlive the run. Split panes whose session is away (a host
not yet connected, another profile) are held as saved and return to their
place when the session does (`panes.Workspace.Follow`); changing the layout
meanwhile makes the changed one the layout to keep.

## Demo mode

Settings, Experimental, Demo mode (`settings.Model.DemoMode`, the controller
preference `demo_mode`) shows the demo conversations in place of the
window's own, without a restart, through the same sidebar, transcript,
composer and right panel. `preview.Demo(locale, host)` builds them
(`internal/desktop/preview/demo*.go`, prose in `preview/demo/<locale>.yml`).

The window has two stages (`shell.stage`): the dataset, the sidebar's,
transcript's and panel's state, the drafts, the opened subagent sessions, the
panes and the panels per conversation. `App.syncDemo` follows the setting at
the start of a frame: it parks the window's own stage and puts a new demo on
the App's fields, or puts the parked one back as it was, drafts and selection
included. While the demo shows, `App.underneath` puts the window's own stage
back for the controller's frame hooks (`BeforeFrame`, `AfterFrame`) and for
`App.Prefs`, so the controller projects, prunes and saves what it always
does and the demo's panels and order are never saved. The Node keeps running.

| While the demo shows | |
|---|---|
| Conversation, composer, sidebar and panel actions | The preview's: they change the demo's memory. `Hooks.Conversation`, `RenderChild`, `WorkspaceProject`, `Overview`, `OpenKnowledge` and the sidebar's `Actions` are not called, so no command reaches a Node |
| A sent message | Plays the demo's reply (`preview.DemoReplies`) |
| Models | One provider, DeepSeek: DeepSeek V4.1 Pro (selected), DeepSeek V4.1 and DeepSeek V4.1 Flash (`preview.DemoModels`, `DemoFamilies`). They exist in the composer's context only; Settings lists the profile's own providers |
| Identification | None, by the user's decision of 2026-11-11: no header tag, toast, badge or title suffix. `kit.SetDemo` marks the frame and `kit.PreviewNotice` and the preview-only hints stay silent. A control either acts on the demo's memory or is absent ([Demo controls](#demo-controls)) |
| Context ring | Reads the tokens used of the model's window, as a measured indicator does (`conversation.Usage.Window`, 200k for the demo's models): `48k / 200k 24%`, the same as its tooltip. The preview, whose usage has no window, keeps its compaction hint |
| Statistics figures and context panel | The statistics package's own, as over a session: its five figures in the context bar, each with its rows on hover, and its context panel in the header (`Hooks.PluginHeader`). `Hooks.DemoConversation` runs after the demo's composer context is built and sets the figures (`composer.State.Metrics`). The controller mounts the package's composer view for each demo conversation shown, without a session, under the presentation `demo/<id>` (`internal/desktop/live/plugin_demo.go`); what the view reads as the composer's state is the state of a session that recorded what the conversation shows (`demoComposer`, `demoStatistics`), every value by a fixed rule from the conversation's `Session.Usage`, its transcript and the model its composer is set to, so the panel draws each section a session's does and the parts add up. From the usage: one token in eight is output and three tenths of the output reasoning, the cached part of the input, the cost with every response priced, the speed as output over its time, the context used of the window. From the model: the model and window of the last request and the compaction budget (the threshold at four fifths of the window, the output reserve). From the transcript: one request a replying turn whose context grows evenly to the context used, each divided into the seven categories the Node counts, which add up to it (`demoCategories`); the tool calls by name (`demoTools`); the subagents with their usage beside the session's, a quarter of the session's tokens for one that ran to an end, half of that for one running or stopped, none for one queued, at the session's price (`demoDelegated`); the compactions that wrote a summary and the last one that ran. The time to the first token is a few hundred milliseconds by the number of turns; the phases are the requests' waiting and output, 1.6 s a tool call and the rest of what the replies took, and add up to the active time. The panel's latest compaction row scrolls the demo's own transcript to its turn (`App.DemoReveal`). No other package is mounted over a demo conversation: theirs would ask the Node. Nothing is asked of a Node for it; the package's bundle is the one another of its mounts already read, and is read from the Node only where none has it. The mounts and their contributions leave with the demo. A profile without the package enabled shows no figures, as a session does |
| Archive prompt | Names the conversation and offers Archive, as the application's does; it archives in the demo's memory |
| Hosts, the rail, Settings, plugin pages, the performance indicator, message display | The window's own, live, but for the usage a plugin page reads (next row) |
| Usage page (the statistics package) | The demo's usage ledger (`preview.DemoUsage`, held by the demo and read through `App.DemoUsage`): a year of model responses over the demo's projects, provider and models, generated from a fixed seed around today, with the demo conversations' own usage counted exactly. Plugin pages are mounted with it as their host's `UsageSample` (`live.Runtime.pluginUsageSample`), which answers `usageSources` (one source, this device), `readUsageChoices`, `readUsageInventory`, `watchUsage` and `refreshUsage` in the Node's shapes and asks no Node. `Hooks.DemoChanged` runs once when the switch is flipped or the demo is written in another language (`live.Runtime.demoChanged`). A page that may read usage (`pluginUsesUsage`: a workspace or settings page of a package that declares `usage.read`) is disposed and mounts again on the next frame that shows it, so an open Usage page redraws at once in its initial state over the other ledger: no project, provider or model filter, the default range, the first page of records, and a header without its old filters. Disposing closes the mount's host, which ends its calls waiting in `nextUsageSources` and `nextUsageChange` and its observers; the new mount takes the bundle the old one read (`pluginViewState.remounts`), so the Node is asked nothing. The page is remounted rather than given the other ledger under its living state because its filters are the package's own state, which the host cannot clear. Every other mount stays, the statistics package's figures over a composer and its header popover among them, and only has its host answer from the other ledger (`plugins.Host.SetUsageSample`) |
| Activity centre and notification centre | The demo's own notices (`preview.Demo`'s `Workspace.Notices`): the bell lists them, the centre opens, and read, Mark all as read, dismiss and Delete read change the demo's memory. The controller's `ReadNotice`, `ReadNotices`, `DismissRead` and `DismissNotices` hooks are not called |
| Terminal and Browser tabs | Offered, as in the live window: the launcher and "+" list Browser, Terminal, Files and Git. The terminal shows the demo's screen and takes no input; the browser shows the demo's page and loads nothing ([panel](panel.md)). No shell starts and no network is reached. The per-conversation panels keep the demo's tabs in the demo's stage |
| Sidebar menus | Every item acts on the demo in memory or is absent; none is left silent ([sidebar](sidebar.md#project-menu)). `sidebar.State.Sample` gives the project menu the demo's project page and terminal (`App.demoProject`, `App.demoTerminal`) |
| Rail Files and Git pages | The demo project's sample resource (`App.project` returns the demo's `panel.Project`, whose `Actions` is nil): its Files and Git actions change the demo's tree, index, branches, stashes, tags and remotes in memory (`panel/sample.go`), and what needs a Node, the network or the system is not offered ([Demo controls](#demo-controls)) |
| Project editor and appearance | Saved to the demo's memory. Open folder and the editor's Browse button open the file picker over the sample catalog (`picker.Catalog`), as the preview does, without the preview's note; the folder chosen is added to the demo |
| Session overview (header) | `preview.Overview` over the demo conversation, not `Hooks.Overview`. An output or a source opens the demo's file in the panel (`preview.Asset`); a cited page opens a browser tab of the panel at its address, as a reply's link does (`App.sampleLink`) |
| Questions and approvals | Answered and decided in memory, every kind: a plan review's Continue and Execute, a form's values and an authorization link (no page opens). A request read again or sent again changes the demo's rows |
| Editing a sent message | Save replaces the message in memory and sends it again: the turns after it leave and its reply plays. New chat from a selection opens a demo chat with the quote as its draft |
| Links standing alone in a reply | The demo's own resolver (`preview.Embed` on the demo's `conversation.State`): its files as cards, its pictures and a tool's pictures as thumbnails. The controller's resolver is not called |
| Composer plugin modes, pasted files | No plugin mode is offered; a pasted or dropped file plays a transfer and is not read |
| Queue | Edited, reordered and sent in memory. A subagent's session (`Session.ReadOnly`) has its read-only tray and no composer |
| Shortcuts `conversation.stop`, `composer.focus`, `composer.models` | Act on the demo's composer; the model controls list the demo's models |
| Close and quit prompt | Considers the window's own documents only: `live.Runtime.UnsavedDocuments` reads the controller's projects, never the demo's stage, and Save all saves those |
| In-app notice toasts | A real notice still shows as a toast, without Details: its conversation waits behind the demo, so nothing is opened or switched (`live.Runtime.PresentNotice`) |

The demo is written again when the window's language changes between en and
zh-CN; another language reads the English one.

### Demo controls

A control over the demo does in memory what it says, or is absent. None is
left silent, none names the demo, and none reaches a Node. The preview keeps
every control and tells `preview_notice` instead.

| Control | In Demo mode |
|---|---|
| Send, Guide, Queue and the queue's rows | The demo's reply plays; the queue is edited, reordered and sent in memory |
| Stop (`conversation.stop`, the composer, Escape) | Ends the reply that plays (`preview.Stop`) |
| Retry | Runs the turn again: its reply plays from the start (`preview.Retry`) |
| Edit a sent message, Save | Replaces the message and sends it again (`preview.Resend`) |
| Fork session, Create worktree (a turn's menu) | Copies the conversation up to the turn beside itself and shows the copy, in a new worktree of the project for Create worktree (`sidebar.State.Fork`). A chat without a project has Create worktree disabled (`App.demoControls`) |
| Rewind | Asks as the application does, then drops the turns after the turn (`preview.Rewind`) |
| Compact | Adds a compaction turn that runs and completes (`preview.Compact`) |
| Undo on a change card, Undo all | Opens the History tab and asks over it, then marks the file or the card undone (`preview.Undo`). The History tab's own Undo all asks, then drops the checkpoint |
| Review on a change card, a file of the Git badge | Opens the diff in the panel |
| Approvals, questions, plan reviews | Decided and answered in memory |
| A goal's Pause, Resume and End | Change the badge and its buttons, or take it away (`preview.GoalAction`) |
| Stop on a background command | The row leaves |
| A service's port | Opens the panel's browser |
| New worktree, Add project, Manage teams (the composer's location and team menus) | A worktree added to the conversation's project and chosen for it; the project editor; Settings, Teams |
| Reconnect | Connects at once |
| Voice input | Plays the recording on the clock and ends with the demo's sentence in the draft (`dictation.text` of `preview/demo/<locale>.yml`). No microphone opens |
| A pasted or dropped file | Plays a transfer; the file is not read |
| Copy, Quote, New chat from a selection | Clipboard; the draft; a demo chat with the quote |
| Read aloud (a turn's footer) | The button shows reading and Stop while it is pressed again. No voice speaks: reading through the system's voice is the controller's |
| Read aloud in the selection dialog | Absent in a preview window, which has no voice; a live window's reads the text through its own voice |
| Export (a turn's footer, a session's menu, a document's card) | Absent: the demo's conversations are in no file |
| A file card, a file of the overview | Opens its tab in the panel. A file without a native preview (a PDF, a deck) opens a tab that names it and its kind, also on the platform-modifier click. Open in another app and Save as are absent: the file is on no disk |
| The image viewer's Copy, Save and Open | Absent |
| A link of a reply, a cited page | A browser tab of the panel at the address, which loads nothing; the system browser with the platform modifier |
| Files: new file and folder, rename, move, copy, cut, paste, trash | The demo's tree changes (`panel.Project.sampleFile`). Open in another app, Reveal, Download and Insert into conversation are absent; Find is disabled over the read-only view |
| Git: stage, unstage, discard, commit and its variants, undo commit, cherry-pick, switch, new, rename, delete and merge branch, tags, stashes, add and remove remote, ignore, fetch, pull, push and sync | The demo's index, history, branches, tags, stashes and remotes change (`panel.Project.sampleGit`); the network actions only settle the branch's tracking state |
| Git: Clone, Manage worktrees, Git output, View stash, Rebase and a branch's Create worktree | Absent: they clone over the network, list a Node's worktrees and its commands' output, or would show nothing on the demo's history |
| Git: Continue and Abort | Disabled, as in a repository with no operation under way |
| Sidebar: New session, the chats' "+", Sort, Add project, a group's fold, Show more | As in the preview: a new chat, the order chosen, the project editor |
| Sidebar: a session's Rename, Pin, Mark read or unread, Archive, Delete, Copy, Fork; drag to reorder | In memory, Archive and Delete after their confirmation ([sidebar](sidebar.md#project-menu)). Export, Open folder and New window are absent |
| Sidebar: a project's menu | Every item ([sidebar](sidebar.md#project-menu)) |
| Rail: the host picker and its right-click menu | The window's own hosts. Host information, Add project, the port forwards and Update open as they do. Edit is the controller's: a live window keeps it, a preview window's demo has none |
| Search palette | A session opens, a project opens its page, a host its page |
| Bell and notification centre | The demo's notices, read and dismissed in memory |
| Rail pages of packages (next table) | The window's own, over the real Node |

A rail or Settings page of a package is mounted on the window's Node and
shows the profile's own data while the demo shows. Only the statistics
package's Usage page reads a demo source (`UsageSample`).

| Package page | What it reads | A demo source would take |
|---|---|---|
| Usage (`statistics`) | The demo's ledger | Done |
| Activity Board (`activity-board`), Task Plaza (`task-plaza`) | `readActivityCatalog`, `readActivityPreviews`, `nextActivityHosts`, native operations bound in `live/plugin_activity.go` | Three operations answered from the demo's conversations in `live`; Task Plaza's details also mount a conversation, which must resolve to a demo one |
| Scheduled tasks (`scheduled-tasks`), Reminders (`reminders`) | Package storage (`listKeys`, `getValue`), the dispatch lists and results through the request calls, the project, model and team catalogs | A sample hook for the host's reads keyed by command kind, the same for the request calls, demo records in each package's own shape, and a rule for the pages' saves |
| Databases (`databases`) | `listDatabases`, `listSsh`, `browseDatabase`, and rows through `query_database` | The same hooks, plus canned results for free SQL and a refusal for edits |
| SSH (`ssh`) | `listSsh`, `listSshTerminals`, `browseSshDirectory`, terminal inspection and transfers | The same hooks for the profiles; its terminals need a screen of their own |
| Browser, External Browser, Memory, Search (Settings pages), the games | Their own settings and storage | Not examined |

`live.Runtime.demoChanged` remounts only the pages that read usage; a wider
demo source needs its predicate widened with it.

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
translations and rebound keys; menu commands scoped as their keys, behind
the sidebar's dialogs too.
`internal/desktop/shell/frame_test.go`: ⌘⌥B, ⌘P, ⌃⇧G and ⌘W on the panel;
⌘B and preferences round-tripping; the side column of every page, its
toggle and ⌘B per page, no motion between pages, the saved choice across
pages without a column and an earlier file (`testSideColumns`); the
header's and the rail's icon buttons' boxes, gaps and hover fill and the
disabled details toggle (`testIconButtons`); the header's two regions
(`testHeaderSplit`, with a right title-bar inset of 0 and 138, a run active
and none, the column open and closed): the toggle alone at the far right,
the session status then Search and the overview at the main region's end,
left of the split, the band's
line on the column's border also after a resize, the tabs in the band with
no strip under it, the details and metrics titles in the band without a
close button, and the toggle disabled with nothing to show
(`SAILRY_UI_SHOTS=<directory>` saves its zh-CN dark frames); ⌘K search opening a conversation;
the activity centre opening a conversation and the knowledge viewer
paging; `ParseSource`, `Filter`, `Next`, `MoveFeature`.
`internal/desktop/shell/strip_test.go` (`TestHeaderStrip`): the strip's
groups and +N; its place left of Search with the column open and closed
and at the region's end without those actions (`testStripPlace`); folding
to +N beside the column without reaching over the title
(`testStripFolds`); and the header alone and over a split asking for no
frame at rest (`testStripAtRest`).
`internal/desktop/shell/panes_test.go` (`TestPanesRender`): Search and the
overview in the band for a conversation alone and in each conversation
pane's header in a split, each acting on its own conversation, ⌘F on the
focused pane's, none for a terminal pane (`testPaneActions`) or a
conversation without a turn (`testPaneWithoutTurns`).
`internal/desktop/shell/knowledge_live_test.go` (`testLiveKnowledge`, run by
`TestActivityCentreReadsAndOpens`): an invalid link never reaches the hook, a
notice is marked read and hands over its citation, the live page's texts, and Previous and Next
at the ends and while loading.
`internal/desktop/live/knowledge_test.go` (`testKnowledgeViewer`, run by
`TestNoticesProjection`): which notification text becomes a notice's link,
the request of a link, the answers accepted, the toast of each fault and
paging by re-reading. `TestKnowledgeUIIntegration` (integration) publishes a
real notification citing an indexed chunk, clicks it in the activity centre
and finds it read on the Node with the viewer on that chunk.
`internal/desktop/shell/pages_test.go`: the rail opens pages, not tabs; a
plugin's page has no session sidebar or toggle and starts at the rail; the
Files and Git page and tab share a project's state, another project
keeps its own; a live window's pages show the hook's project, are
unavailable only without its worktree and offer New project without a
project. `internal/desktop/live/rail_pages_test.go` drives both pages
against a real Node.
`internal/desktop/shell/sidebar_test.go` and `shell_test.go`: the sidebar and
Phase 1–2 screens.
`internal/desktop/live/demo_mode_test.go` (`TestDemoMode`): Demo mode over a
real Node: the settings' order, the switch, the swap and the way back, no
command sent by the demo's actions, no notice text, the model shown and the
preference saved; the sidebar's project and session menus and the
transcript's Stop and Retry over the demo in that window (`menus and
actions`), each changing the demo's memory with nothing sent; the statistics package's figures with their cards and its
context panel over a demo conversation (`statistics`: what `demoStatistics`
and `demoComposer` record for a transcript, the parts adding up), every
section and fold of the panel, the subagents' usage, the compactions and the
latest compaction's row scrolling the demo's transcript, with no view or
statistics command sent and nothing left after the way back; the Usage page's calls answer from the demo's ledger
without a command and from the Node again after the way back, the waiting
call moving over. `internal/desktop/shell/demo_actions_test.go`
(`TestDemoActions`) and `project_menu_test.go` (`TestProjectMenu`): each
control of [Demo controls](#demo-controls) over a preview window's demo;
`internal/desktop/panel/sample_test.go` (`TestSampleActions`): the demo's
Files and Git actions and what its Git menus leave out.
`internal/desktop/preview/demo_test.go` (`TestDemo`): the
demo shows every value of the view model's enumerations and sets every
field, both catalogs have the same keys, and the models' names; its `usage`
step reads the ledger over every range, filter, grouping and page of
records. `internal/desktop/plugins/host_usage_test.go` (a step of
`TestHostSDKCalls`): the usage calls with a sample, with another and with
none.

## Deviations

| Deviation | Reason |
|---|---|
| The composer floats over the transcript's end rather than standing under the transcript (user decision 2026-11-12) | The reference's column ends the transcript's scroll bar where the composer begins; see `docs/conversation.md` |
| Breadcrumbs in the header | The plan asks for them; the reference shows a title |
| Header icon buttons are 28 with a 15 icon, the rail's icon is 15 | Kit's accepted small button (docs/components.md); the reference's header buttons are 24 with a 14 icon and its rail icon is 16 |
| The details toggle is disabled while the window has no room for the panel | It cannot act then; it used to dim and still take clicks |
| The right column's tabs or title are in the header band, and the details toggle is its only close control (user decision 2026-11-11) | The reference gives the column its own strip row and each details page a title row with a close button. One band keeps the toggle in one place and gives the column its full height |
| The header's session status shows on the conversation page only, in the main region left of Search and the overview; the details toggle is alone at the far right (user decision 2026-11-11) | Its existing rule for the page; the status belongs with the page's actions, and the toggle keeps one place |
| In a split, Search and the session overview are in each conversation pane's header and not in the band (user decision 2026-11-11) | They act on one conversation, and the band names none over a split |
| The strip's room is computed, with the plugins' contributions and the tag measured from the last frame | A MyGo row cannot ask a child to show less before the title is cut; the strip must choose its own count for the room |
| One pane's overview popover closes when another pane's button is pressed | `ui.PopoverBase` closes on a press outside it; the two states are separate and the pressed one still opens |
| Window dragging over the band is not tested | The headless backend's `startDrag` does nothing; the segment and the side row carry no interactive flag |
| Windows and Linux title-bar insets are simulated, not run | `ui.Tester.SetTitleBar`; only macOS was run |
| The resize handles are drawn strips | `ui.Split` cannot close a pane to nothing |
| Preferences are a JSON file beside `window-state.json` | MyGo's `WindowState` has no fields for the app's own state |
| The notification center page, Activity board and Market | Later phases; Activity and Market show `unavailable`, Market not yet in Settings |
| ⌘/ opens Settings | No Shortcuts section before phase 5 |
| The rail's More rows are a popover, not a menu | Rows hold a pin button and drag to reorder |
| System shortcuts register at launch only | Keys changed in Settings take effect at the next launch |
| Demo mode shows only the statistics package's controls in the conversation's header and context bar | The other packages' mount over a Node's session and ask it, which a demo conversation is not. The statistics package reads the composer state alone, which the host builds from the demo conversation's usage |
| A header popover does not open on hover | The source opens one whose declaration sets `hover` while the pointer is over it. No official package declares it; the press opens here |
| The selected text is the preview's sample | Reading another app's selection needs the platform's accessibility APIs and a Node |
| About opens Settings, which has no About section | Settings sections beyond Appearance, Providers and Plugins are later phases |
| Save (⌘S) and Logs are disabled | The preview has no file editor or log folder (the reference disables Logs without one) |
| Help lists Welcome to Sailry and Selected text | The preview's way to the first launch's guide and the system shortcut's dialog |
| ⌘., ⌘L, ⇧⌘M, ⇧Tab and the conversation's Escape targets are not bound | The live conversation's controls (`conversation/live/shortcuts.rs`) come with the live session |
| The View menu's features are those at launch | The menu is set once; rail reordering does not rebuild it |
| A live citation always opens in the knowledge viewer | The reference opens a worktree citation with a line position in the conversation's file panel; the coverage rows name the viewer |
| The knowledge viewer shows no spinner while a page loads | The accepted layout has none; Previous and Next are disabled instead |
| A live notice opens the knowledge viewer when its text details are one citation link | The protocol's notifications have no link field and the reference opens citations only from chat links. `live.Notices` sets `Notice.Link` when a notification's `text` details, trimmed, are exactly one `sailry-knowledge://` link the Node can read; any other text stays details only. No bundled package publishes such a notice yet |
| The unsaved-documents prompt has Save all | The reference offers only Discard and Cancel; saving from the prompt keeps the drafts without a second trip |
| A quit without unsaved documents confirms only while the local Node runs a task or a terminal | The reference always confirms. Nothing is lost by quitting an idle app, and work on a remote Node goes on without this controller |
| Save all in a window that is not the asking one saves in that window's next frame | Each window's Runtime writes its own files on its UI frame; the prompt polls every 100 ms while saves are under way and only then |
| Details on a toast for a notice without a conversation opens the notification centre without expanding that notice | The centre's expansion state belongs to its page |
| Plugin notifications are not gated by the per-kind notification switches | The existing Notifier rule: the three switches name approval, input and task outcomes |
| A click on a notice without a conversation or citation only reads it; Details on its toast opens the centre | The reference opens the publishing package's page (`shell/activity/attention.rs`). The notice view model does not carry its package, and a package may have no page; a reminder's text is whole in the row and the centre |
| A background notice the system channel did not take is shown at the next focus, not while the window is in the background | The reference adds the toast at once. A MyGo toast's time runs in the background, so a toast added there is gone before its user returns |
| A remote host's notices that exist when its connection is first read raise no toast | They were raised for whoever was connected then; the bell and the centre list them |
| The `terminal` notice details are not read | Nothing on either side writes them (publishing accepts `text` only); the reference's reader is unreachable. The protocol type stays |
| With the live `OpenLink` set, `#heading` links in replies no longer scroll | MyGo hands every link to `OnLink` once it is set; knowledge links need it |

## B3 live wiring

`live.NewWithPreferences` composes the accepted shell over the already started
shared Client. `viewmodel.Data` and Workspace DTOs are neutral presentation types;
preview aliases retain the explicit preview constructor and its interactions.
`App.Hooks` injects refresh, folder selection and notice operations. Background
callbacks only enqueue work. `App.View` drains it on the UI thread; `SetUpdate`
binds MyGo Window.Update to schedule the frame. The live path never constructs
fixtures, advances preview streams or simulates transfers, dictation or host
metrics. Models, tools and later-phase settings stay unavailable. A file or
diff the live workspace opens (`workspaceProject.showItem`: a search result, a
new file, a commit) shows on the rail's Files or Git page when that page is
shown, and in the panel's tab otherwise.

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
observes the local Client and the controller of every connected host
(`Notifier.SetNetwork`, following `Network.onChange`) independently of frames,
suppresses categories from controller preferences and delivers each new notice
once, through one channel. A host's notices from before this application first
read them are not news. While a window is focused that window takes the notice:
`Runtime.PresentNotice` adds a Kit toast with a Details action, which opens the
notice's conversation, or the notification centre for a notice without one. A
notice of a conversation that window is showing has no toast and no system
notification. With no focused window the notice is a system notification, which
the first notification preference switches off; that switch does not silence
in-app toasts. A background notice the system channel did not take, because
that switch is off or the system refused it (`Notifier.Undelivered`), is kept,
at most `NotificationLimit` of them, and shown as a toast by the next window
that takes the focus (`Notifier.Returned`), unless it was read or dismissed
meanwhile. The Notifier's UI scheduler remains alive when all windows are
closed; it closes before Client shutdown.

A notice carries its package's `presentation`. The toast and the system
notification are headed by the package's heading in the window's language
(`shell.NoticeHeading`: `locales[language]`, else `heading`) above the notice's
title and message; a `persistent` notice's toast stays until it is closed. The
bell and the centre keep the notice's own title and use the heading only for a
notice without one. System notices (`power`, `diagnostic`, `system` details)
keep the application's headings. The centre's Source names the notice's own
host (`App.noticeSource`), not the local one.

Closing a window or quitting asks once before something is lost.
`cmd/sailry-desktop/closing.go` owns the decision: MyGo's `Window.OnClose` and
`Application.OnBeforeQuit` prevent the close, and the prompt's answer repeats
it with the guard passing. `App.ConfirmClose` (`closing.go`) draws the prompt
in the window that asks; `live/closing.go` supplies the window's unsaved
documents (`panel.Project.Unsaved`), saves them through the ordinary file
write and reports running local work. With unsaved documents the prompt names
them and offers Save all, Discard changes and Cancel. Save all closes only
after every save succeeded; a failed save tells its usual toast, closes the
prompt and leaves the window and the drafts as they were. A quit, which
closing the last window is, covers every window's documents in one prompt and,
without documents, confirms only while the local Node runs a task or a
terminal. Closing one of several windows without drafts never asks. An ending
OS session cannot wait and passes. Installing an update is refused with a
toast while any window has unsaved documents; its restart then passes the
guard.

Profile preferences are one shared Store across windows. Each window synchronizes
its revision before rendering; disk writes are serialized immutable snapshots,
and a stale captured version cannot overwrite a newer edit. Welcome state is in
`desktop/preferences.json`. `ui-state.json` still restores column widths,
collapsed projects, rail state, tabs and pane layout; its legacy welcome field is
used only by the preview. Closing a live guide persists the profile choice.

The notification centre uses existing Kit page/card/segmented controls
([Notification centre](#notification-centre)). Live
host views hide preview-only notes; B7 supplies their real facts and metrics.
In the preview, terminal panes retain the accepted static placeholder and never
start a process.

### B10 live terminal panes

In live mode a terminal pane builds its tab's Node terminal through
`panel.TerminalActions.Pane` (see [panel.md](panel.md#b10-live-terminal-tabs));
the pane's ID is the tab's, which is the Node terminal's. ⌘D and ⌘⇧D call
`TerminalActions.Split`, which starts a Node terminal in the pane's worktree,
and drop its tab beside the pane within the 9-pane limit. Closing a pane, after
the existing confirmation, only removes it from the split: the terminal keeps
running in its tab. Closing the tab itself ends the shell, after the same
confirmation. A pane exists while any conversation's panel has its terminal's
tab (`App.HasPanelTab`), so a split keeps a terminal whose conversation is not
the selected one. A tab the live inventory prunes, because its Node closed
the terminal or no longer reports it, takes its pane out of the split through
`followPanes`. Saved `Prefs.Panes` and `Prefs.PanelTabs` come back at start-up
and reattach when first shown. A shell that exits closes its terminal, so its
tab and pane leave the same way.

Local terminals are core-owned: ⌃`, ⌘D and ⌘⇧D always start a core terminal,
whatever packages are enabled. A package's `terminal` renderer, creation
contribution and shortcut are not mounted
(see [panel.md](panel.md#b10-live-terminal-tabs)).

Deviation: the reference lets a package own terminals and routes a split
through the package's creation contribution. Here local terminals are core by
user decision (2026-11-10), like Files and Git; SSH terminals remain
package-owned and are opened from the package's own page, so they are not
split panes.

Project ⋯ › New terminal (`sidebar.Actions.NewTerminal`) and the project
overview's New terminal (`ProjectActions.NewTerminal`) start a terminal in the
project's worktree and open its tab. The overview's Terminal records list the
project's terminals its Node has not closed (`ProjectActions.Terminals`) and a
row shows that terminal's tab (`ProjectActions.OpenTerminal`).

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

## B13 system shortcuts

`cmd/sailry-desktop` registers `App.SystemShortcuts` with the system on focus
and after every preference change, so a rebound key re-registers and a cleared
key unregisters. A refused registration toasts `shortcut_conflict` once per
accelerator through `Runtime.ReportShortcutConflict`.

`system.activate` shows and focuses the active window, then opens the search.
`system.selection` calls `Runtime.CaptureSelection` before the window comes
forward: the front app's selection is read off the UI thread, then the window
activates and shows the text through `App.ShowSelection`, or toasts
`selection_permission`, `selection_empty`, `selection_large` or
`selection_unavailable`. Help › Selected text reaches the same reader through
`Hooks.CaptureSelection` without activation. The text is refused over 65536
bytes or with a NUL, and secure text fields are never read.

Help › Logs is enabled in a live window whose process installed a log
(`Runtime.SetLogFolder`, `Hooks.OpenLogs`). It sends `help.logs`, which opens
`<profile>/logs`; a missing folder toasts `files_open_failed`. The preview and
a process without a log keep the item disabled.

Deviations from the reference:

| Deviation | Reason |
|---|---|
| Selection capture is macOS only (Accessibility through purego); other systems toast `selection_unavailable` | The UI Automation and X11 readers are not ported |
| Help › Logs opens the profile's `logs` folder with the system's file manager | The reference opens the current file in Console on macOS; the folder also holds the rotated files |
