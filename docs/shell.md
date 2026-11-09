# Shell

The main window's frame: the header band, the feature rail, the sidebar
column and the right panel beside the page, their motion and resizing,
the window's shortcuts and native app menu, the search, the activity
centre, the knowledge viewer and the UI preferences kept across launches
(sailry-public `shell.rs`, `shell/{frame,frame/*,rail,rail/*,header,
splits,limits,search,search/*,activity,activity/*,knowledge,knowledge/*,
shortcuts}.rs`, `shortcuts.rs`, `app_menu.rs`, `activity/*.rs`).

## Package and files

`internal/shell`

| File | Holds |
|---|---|
| `shell.go` | `App`, `View`, the frame: columns, `Motion`, resize handles, the page and its actions |
| `header.go` | The header: brand, sidebar toggle, breadcrumbs, activity strip, preview tag, details toggle |
| `rail.go` | `Features`, `MoveFeature`, the rail, More, the host picker |
| `pages.go` | The rail features' pages, `Navigate`, `ClosePage`, the selected project's `panel.Project` |
| `shortcuts.go` | `Shortcuts`, `CloseFocused`, `KeyText`, the window's key handling, the menu's commands (`Do`) |
| `menu.go` | `Menu`, `Accelerator`: the native app menu |
| `search.go` | `Choice`, `Group`, `Filter`, `Next`, the search overlay |
| `activity.go` | `NoticeAge`, `SortNotices`, the activity centre and the header's strip |
| `knowledge.go` | `Source`, `ParseSource`, the knowledge viewer |
| `prefs.go` | `Prefs`, `LoadPrefs`, `SavePrefs`, `ClampNav`, `PanelFits`, `ClampPanel` |

## Entry point

```go
app := shell.New(preview.Load())
app.SetPrefs(shell.LoadPrefs(userData)) // at start
shell.SavePrefs(userData, app.Prefs())  // on blur, close and quit (cmd/sailry)
mygo.NewWindow(mygo.WindowOptions{StateKey: "main", Content: ui.View(app.View), ...})
```

`cmd/sailry` also imports `github.com/egoist/mygo/locales/zh-Hans`, so
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
Search and Settings at the bottom. Conversation is fixed first and Market
last. More is a frosted popover to the right listing every feature with
a pin (`feature_pin`/`feature_unpin`); rows drag to reorder
(`MoveFeature`, a 4 DIP primary line). Every feature opens an
independent page in the main column, as the reference's `Page`
destinations do: Files and Git over the selected conversation's project
(the same `panel.Project` the panel's tabs show, `docs/panel.md`),
Activity and Market as the reference's preview shows them (`unavailable`).
The panel shows on the conversation page only; selecting a conversation
over a page goes back to it, and `ClosePage` closes the page shown. Hosts: a frosted menu with the hosts (check on this one,
`sidebar_host_update`), `host_add`; a dot when one has an update; its
right-click menu is the system's (`workspace_host`,
`connection_action_edit`, `project_add`).

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
(Minimize, Zoom, Bring All to Front) and Help (Website, Feedback, Logs).
The system's items are MyGo roles. The app's items send their command's
ID, which `cmd/sailry` passes to `App.Do`.

The menu takes its items' keys before the window on every platform, so a
command from the menu runs in the next frame scoped as its key would be:
Close Tab closes what ⌘W closes where the focus is (`closeFocused`, by
`Context.FocusWithin` of the panes' handles), and the window's commands
wait while the search, the knowledge viewer or the project editor shows,
as a modal dialog holds the window's shortcuts. This is the only scoping
added over MyGo's own.

## Search

⌘K or the rail: a frosted dialog 480 wide near the top. Root rows: Chats,
Projects, Hosts, Settings (folders) and the features. A folder lists its
items with a check on the current one. `Filter` keeps rows whose label or
keyword holds the query, case folded, in order, dropping empty groups;
`Next` moves round. Enter opens a folder or runs a row, Escape clears the
query then closes, Backspace on an empty query goes back, `reference_back`
too. Empty: `connection_search_empty`.

## Activity centre

The bell, a red dot while notices are unread, opens a frosted popover to
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
rows. The pointer resting on one shows its title and project in a frosted
card below it.

## Knowledge

`ParseSource` accepts `sailry-knowledge://<collection>/<chunk>?revision=<r>`
only, as the reference. The viewer is a frosted dialog up to 800 wide:
`knowledge_source`, the title, `host · path · Lines a–b`, the page in mono,
Previous, Next and Close. The preview pages its citation in memory; the
activity centre's knowledge notice opens it.

## UI preferences

`Prefs` keeps the sidebar's width and openness, the panel's width,
openness, tabs and active tab, the projects closed in the sidebar, and
the rail's order and unpinned features, in `ui-state.json` in MyGo's user
data folder (beside `window-state.json`, which `StateKey` keeps the
bounds in). `LoadPrefs` falls back to the defaults and clamps widths.

## Wiring a backend

Notices come from the activity stream (`activity::Notice`); reading one
sends `MarkNotificationRead`. The search's sessions come from the
client's recent sessions; its hosts from the live hosts. Knowledge pages
come from `ReadKnowledgeCitation` with the checks of `viewer.rs`. Rail
pins and order belong in the preferences beside the rest of `Prefs`.

## UI state vs backend data

| UI state (`App`) | Backend data |
|---|---|
| Page, column widths and openness, rail order and pins, the search's folder, query and row, popovers, the viewer's page | Hosts, notices and their read state, conversations, knowledge pages |

## Tests

`internal/shell/shortcuts_test.go`: ⌘W in a focused panel tab, a focused
Files or Git page and with nothing closable focused; Escape closing only
the popover; a text input keeping ⌘A and ⌘Z; the menu's items, keys and
translations; menu commands scoped as their keys.
`internal/shell/frame_test.go`: ⌘⌥B, ⌘P, ⌃⇧G and ⌘W on the panel;
⌘B and preferences round-tripping; ⌘K search opening a conversation;
the activity centre opening a conversation and the knowledge viewer
paging; `ParseSource`, `Filter`, `Next`, `MoveFeature`.
`internal/shell/pages_test.go`: the rail opens pages, not tabs; the
Files and Git page and tab share a project's state, another project
keeps its own.
`internal/shell/sidebar_test.go` and `shell_test.go`: the sidebar and
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
| Global ⌃⌥Space is not registered | System shortcuts are phase 6 |
| About opens Settings, which has no About section | Settings sections beyond Appearance, Providers and Plugins are later phases |
| Save (⌘S), Find in Conversation (⌘F) and Logs are disabled | The preview has no file editor, find bar or log folder |
| ⌘., ⌘L, ⇧⌘M, ⇧Tab and the conversation's Escape targets are not bound | The live conversation's controls (`conversation/live/shortcuts.rs`) come with the live session |
| The View menu's features are those at launch | The menu is set once; rail reordering does not rebuild it |
