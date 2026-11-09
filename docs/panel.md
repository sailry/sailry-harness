# Right panel

The column right of the conversation: one flat strip of tabs, each an
open item, over the active one, or the launcher when none is open (sailry-public
`resources/*.rs`, `navigation_tabs.rs`, `resources/launcher.rs`,
`preview_files.rs`, `preview_git.rs`, `hunks/render.rs`). The frame owns
its width, motion, toggle and ⌘W (`docs/shell.md`).

## Package and files

`internal/panel`

| File | Holds |
|---|---|
| `model.go` | `Kind`, `Resource`, `Resources`, `Tab`, `State`, and the rules `Opened`, `Closed`, `Overflow` |
| `project.go` | `Project`, `FileView`, `GitView`, `Projects`: open files and diffs per project; the split |
| `panel.go` | `View` and `Page`, the two hosts: the strip, the launcher, the file tree and file view, the change list and hunks, checkpoints, documents |
| `tools.go` | The browser and terminal tabs' chrome, per tab: the browser's history and address, the terminal's folder, resets and find bar; `State.Find` |

## Entry point

```go
s := panel.New(data.Workspace)
ps := panel.NewProjects(data.Workspace)
pr := ps.Of(project.ID)               // nil: no project, the hosts ask for one
panel.View(c, s, pr, shell.KeyText)   // the tab host; KeyText writes the launcher's shortcuts
panel.Page(c, panel.Files, pr)        // the page host, in the main column
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

- Strip: one flat row, no tabs inside tabs. 40 DIPs, tabs 28 high up to
  180 wide with the item's type icon and title (a file's or diff's name, a document's title); the active tab in
  `tab_active`; the close button shows on hover or on the active tab. It
  scrolls sideways; past `Overflow` (4) tabs a chevron (`navigation_tabs`)
  lists all with a check; "+" lists every resource with its shortcut and
  each document; Files and Git open the file or diff shown last. Opening
  an open item focuses its tab. Menus are `kit.MenuButton`. The pages'
  strips draw the same tabs, without "+" and the chevron.
- Launcher: `details`, then 40 DIP secondary rows up to 440 wide: icon,
  label, Kbd shortcut. Order: Browser ⌘T, Terminal ⌃`, Files ⌘P, Git ⌃⇧G,
  History, Docs.
- Files: the tab's file in the main area, the tree in the side list
  selecting it, in the page and a file tab alike. The tree has no
  title and starts at 8 DIPs (rows 28, 16 per level from 8). File view
  toolbar: Save, Undo, Redo (disabled; edits stay in the view), Find, and
  Document | Source for markdown (`kit.Markdown`'s `Document` variant:
  MyGo's markdown in the app theme, code blocks in MyGo's editor). Text in MyGo's editor (`kit.Code`, editable:
  highlighted, line numbers, mono 13 at 1.6, the caret showing, trailing line breaks left out). Status bar: path (right-click: Copy path),
  `Ln 1, Col 1`, UTF-8, language.
- Changes: the tab's diff in the main area, the change list in the side
  list selecting it, in the page and a diff tab alike (list: `changes`, Stage all disabled
  with `git_actions_preview`; rows: file icon, name, status letter in
  warning/success/danger). The diff is MyGo's diff viewer
  (`kit.Diff`), unified as Rust's content diff view: highlighted, old and
  new numbers, word changes, and on the change under the pointer Revert
  and Keep (`editor.DefaultHunkActions`) and Stage. Commit box: message, Commit (disabled),
  `git_preview_only`. Status bar: path, `+N −M`.
- History: a card per checkpoint: the turn, its age, the files, `+N −M`
  and `turn_changes_undo_all`.
- Documents: a tab per document, rendered.
- Artifact: a file's preview; a picture is `kit.Image` (a Skeleton while it
  loads) that opens `kit.ImageViewer`, whose Copy, Save as and Open toast
  `preview_notice`.
- Browser (`browser/view.rs`, `browser/shortcuts.rs`), per tab: Back and
  Forward through the tab's history (disabled at its ends), Reload
  (disabled before a first address), the address (`browser_address`; Enter
  goes, `https://` is added when no scheme is given) and New tab with its
  ⌘T hint, over `resource_browser_preview` under the address shown. ⌘T
  (another tab) and ⌘R (reload) work while the focus is in the tab.
- Terminal (`workspace/terminal.rs`, `terminal/search.rs`), per tab: the
  folder it starts in (the selected project's path, muted), Find, Reset
  (`workspace_reset_terminal`), Close (`workspace_close_terminal`) and New
  terminal, over `workspace_terminal_preview` and `workspace_reset_count`.
  ⌘F with the focus in the tab opens MyGo's `FindBar` at the screen's top
  right (`terminal_search_placeholder`, 420 wide at most, 0 matches);
  Escape or Done closes it. The window routes the native menu's ⌘F
  (`conversation.find`) to `State.Find` while the panel holds the focus.

## Actions

| Action | Effect in the preview |
|---|---|
| Open, choose and close tabs | `State` |
| Choose a file or a change | Opens or focuses its tab, in the panel and the page alike |
| New tab, New terminal | Another browser or terminal tab |
| Back, Forward | The tab's history |
| Close (terminal) | Closes the tab |
| Find (terminal) | The find bar, nothing to match |
| Revert, Keep, Stage a change, Undo changes, Find, Reload, address, Reset | `preview_notice` toast: they write files, git, load pages or start a shell; Reset also counts |
| Copy path | Clipboard |

## Wiring a backend

Replace `preview.Workspace` with the session's resources: files from the
worktree, changes and hunks from `InspectGit`/turn hunks, checkpoints from
`ListFileCheckpoints`. Reject and Stage send `ApplyTurnHunk`, Undo changes
restores checkpoints newest first; outcomes go to toasts
(`hunks_conflict`, `checkpoint_*`). Browser and terminal render inside
the same chrome when their plugins land.

## UI state vs backend data

| UI state (`State`) | Backend data |
|---|---|
| Tabs, active tab; per browser tab the address and history, per terminal tab the folder, resets and find query; per project: shown file and change, open folders, Document/Source, scroll, accepted hunks (until applied), commit draft | Files, changes, hunks, checkpoints, documents |

## Tests

`internal/panel/model_test.go`: tabs open once, closing picks the next,
items are flat tabs (new browser pages, retired kinds dropped), each file
and diff its own tab shared with the project (closing on either side,
another project swapping them), a browser tab's history (going drops the
forward part).
`internal/shell/frame_test.go`: ⌘⌥B opens the launcher, ⌘P a file tab
beside the tree, a file chosen in the tree its own tab, ⌃⇧G and a change
a diff tab (read through the diff viewer's accessible text), choosing a
tab shows its item beside its list, eight tabs show the overflow menu,
Reset counts and ⌘F in the terminal opens its find bar, ⌘W closes tabs
then the panel; collapsed, the header clears the traffic lights.
`internal/shell/pages_test.go`: the rail's Files and Git open pages, the
page's open files are the panel's file tabs, a change chosen in a tab
shows on the page, a diff closed on the page drops its tab, another
project keeps its own; the first frame of a page lays the side list out
at its width.

## Deviations

| Deviation | Reason |
|---|---|
| PDF and other non-native formats: WebView preview, later | MyGo has no native view of them; the card's Open uses the system's app meanwhile |
| Every open item is a tab in one flat strip | The plan asks for tabs; the reference holds one resource and tabs only a resource's documents |
| Checkpoints and documents are tabs | The reference restores checkpoints from a turn and draws documents in a plugin |
| Files and Git keep their state per project | The reference keeps one and resets it when the project changes |
| Each open file and diff is a tab, also on the pages' strips | The plan asks for several files and diffs open at once |
| The split's list is a fixed column past our own drag handle | MyGo's `Split` sizes its first pane, so a first frame jumped |
| Hunks show in the Git tab's main area, not a dialog | One place for the preview's diffs |
| Browser and terminal are placeholders in their chrome | Rendering is later work (MyGo WebView, libghostty-vt) |
| The browser's chrome is core | In the reference the browser plugin draws it; the preview has no plugin UI host |
