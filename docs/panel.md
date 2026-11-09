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
| `project.go` | `Project`, `FileView`, `GitView`, `Projects`: Files and Git state per project |
| `panel.go` | `View` and `Page`, the two hosts: the strip, the launcher, the file tree and file view, the change list and hunks, checkpoints, documents, browser and terminal chrome |

## Entry point

```go
s := panel.New(data.Workspace)
ps := panel.NewProjects(data.Workspace)
pr := ps.Of(project.ID)               // nil: no project, the hosts ask for one
panel.View(c, s, pr, shell.KeyText)   // the tab host; KeyText writes the launcher's shortcuts
panel.Page(c, panel.Files, pr)        // the page host, in the main column
s.Open(panel.Files)                   // the launcher, "+" and the frame's ⌘P, ⌘T, ⌃`, ⌃⇧G
s.Show(panel.Tab{Kind: panel.File, ID: path}) // open or focus one item's tab
s.Close(s.Active)                     // ⌘W; false when no tab is left
```

## Components and hosts

Files and Git are each one component drawn by two hosts. `Page`, the
rail's independent page in the main column, shows the whole component
(`Project.files`: the file view beside the tree; `Project.changes`: the
hunks beside the change list). `View`, the right panel, splits it into
tabs: the tree (`Files`) and the change list (`Changes`) are a tab each,
at most one per project, and a file chosen in the tree opens or focuses
its own `File` tab, a change its own `Diff` tab. Their state is a
`Project` (`FileView`: shown file, open folders, Document/Source, tree
and text scroll; `GitView`: shown change, accepted hunks, commit draft,
list and diff scroll), which `Projects.Of` makes once per project ID and
returns to both hosts: a file chosen on the page is the one the tree
tab selects, a file or diff tab chosen in the panel is the page's shown
one, and another project keeps its own. MyGo binds a `ScrollState` to
one container per build, so while the page and the closing panel both
draw a resource (`State.Paged`), the page owns its scrolling.

## View model

| Type / field | Meaning |
|---|---|
| `Resource{Kind, Icon, Label, Mods, Key}` | A tab and launcher row; `Key` 0 for none |
| `Tab{Kind, ID}` | An open item: `Files`, `Changes`, `History` (ID ""), `File` and `Diff` (by path), `Documents` (by document ID), `Browser` and `Terminal` (by number) |
| `State.Tabs`, `Active` | Open tabs in order, the one shown (kept across launches by the shell) |
| `Project` | One project's Files and Git state, shared by page and tab |
| `preview.Workspace.Files` | `FileNode` tree: folders have `Children`, files `Text` |
| `preview.Workspace.Changes` | `FileChange{Path, Status M/A/D, Added, Removed, Hunks}`; `Hunk{Header, Lines}`; `DiffLine{Kind, Old, New, Text}` |
| `preview.Workspace.Checkpoints` | `Checkpoint{ID, Turn, Time, Files, Added, Removed}`, newest first |
| `preview.Workspace.Documents` | `Document{ID, Title, Path, Text}` (markdown) |

### Layout

- Strip: one flat row, no tabs inside tabs. 40 DIPs, tabs 28 high up to
  180 wide with the item's type icon and title (a file's or diff's name,
  its path as the tooltip; a document's title); the active tab in
  `tab_active`; the close button shows on hover or on the active tab. It
  scrolls sideways; past `Overflow` (4) tabs a chevron (`navigation_tabs`)
  lists all with a check; "+" lists every resource with its shortcut and
  each document. Opening an open item focuses its tab. Menus are
  `kit.MenuButton`.
- Launcher: `details`, then 40 DIP secondary rows up to 440 wide: icon,
  label, Kbd shortcut. Order: Browser ⌘T, Terminal ⌃`, Files ⌘P, Git ⌃⇧G,
  History, Docs.
- Files: the page shows the file beside the 220 DIP tree; the panel shows
  the tree in the Files tab and each file in its own tab. The tree has no
  title and starts at 8 DIPs (rows 28, 16 per level from 8). File view
  toolbar: Save, Undo, Redo (disabled, read-only), Find, and
  Document | Source for markdown (rendered with MyGo's markdown, code
  blocks in MyGo's editor). Text in MyGo's read-only editor (`kit.Code`:
  highlighted, line numbers, mono 13 at 1.6). Status bar: path (right-click: Copy path),
  `Ln 1, Col 1`, UTF-8, language.
- Changes: the page shows the hunks beside the 240 DIP list; the panel
  shows the list in the Git tab and each change's hunks in its own tab
  (list (`changes`, Stage all disabled
  with `git_actions_preview`; rows: file icon, name, status letter in
  warning/success/danger)). The diff is MyGo's diff viewer
  (`kit.Diff`), unified as Rust's content diff view: highlighted, old and
  new numbers, word changes, and on the change under the pointer Revert
  and Keep (`editor.DefaultHunkActions`) and Stage. Commit box: message, Commit (disabled),
  `git_preview_only`. Status bar: path, `+N −M`.
- History: a card per checkpoint: the turn, its age, the files, `+N −M`
  and `turn_changes_undo_all`.
- Documents: a tab per document, rendered.
- Browser: Back, Forward (disabled), Reload, the address
  (`browser_address`) and New tab (another browser tab) over
  `resource_browser_preview`.
- Terminal: its title and New terminal (another terminal tab) over
  `resource_terminal_preview`.

## Actions

| Action | Effect in the preview |
|---|---|
| Open, choose and close tabs | `State` |
| Choose a file or a change | Shown; in the panel, its tab opens or focuses |
| New tab, New terminal | Another browser or terminal tab |
| Revert, Keep, Stage a change, Undo changes, Find, Reload, address | `preview_notice` toast: they write files, git, load pages or start a shell |
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
| Tabs, active tab, address; per project: shown file and change, open folders, Document/Source, scroll, accepted hunks (until applied), commit draft | Files, changes, hunks, checkpoints, documents |

## Tests

`internal/panel/model_test.go`: tabs open once, closing picks the next,
items are flat tabs (files and diffs by path, new browser pages).
`internal/shell/frame_test.go`: ⌘⌥B opens the launcher, ⌘P the tree, a
file its tab, ⌃⇧G the change list, a change its diff tab (read through the
diff viewer's accessible text), eight tabs show the overflow menu, ⌘W closes tabs then the panel.
`internal/shell/pages_test.go`: the rail's
Files and Git open pages, a file chosen on the page shows in the tab and
a change chosen in the tab on the page, another project keeps its own.

## Deviations

| Deviation | Reason |
|---|---|
| Every open item is a tab in one flat strip | The plan asks for tabs; the reference holds one resource and tabs only a resource's documents |
| Checkpoints and documents are tabs | The reference restores checkpoints from a turn and draws documents in a plugin |
| The Files and Git pages hold the whole component, tree or list included | The reference puts the tree and the list in the details column; here the panel stays the conversation's |
| Files and Git keep their state per project | The reference keeps one and resets it when the project changes |
| Hunks show in the Git tab, not a dialog | One place for the preview's diffs |
| Browser and terminal are placeholders in their chrome | Rendering is later work (MyGo WebView, libghostty-vt) |
