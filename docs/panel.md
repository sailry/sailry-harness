# Right panel

The column right of the conversation: a strip of resource tabs over the
active resource, or the launcher when none is open (sailry-public
`resources/*.rs`, `navigation_tabs.rs`, `resources/launcher.rs`,
`preview_files.rs`, `preview_git.rs`, `hunks/render.rs`). The frame owns
its width, motion, toggle and ⌘W (`docs/shell.md`).

## Package and files

`internal/panel`

| File | Holds |
|---|---|
| `model.go` | `Kind`, `Resource`, `Resources`, `State`, and the rules `Opened`, `Closed`, `Overflow` |
| `panel.go` | `View`: the strip, the launcher, files, changes and hunks, checkpoints, documents, browser and terminal chrome |

## Entry point

```go
s := panel.New(data.Workspace)
panel.View(c, s, shell.KeyText) // KeyText writes the shortcuts the launcher shows
s.Open(panel.Files)             // the frame's ⌘P, ⌘T, ⌃`, ⌃⇧G and the rail's Files and Git
s.Close(s.Active)               // ⌘W; false when no tab is left
```

## View model

| Type / field | Meaning |
|---|---|
| `Resource{Kind, Icon, Label, Mods, Key}` | A tab and launcher row; `Key` 0 for none |
| `State.Tabs`, `Active` | Open tabs in order, the one shown (kept across launches by the shell) |
| `preview.Workspace.Files` | `FileNode` tree: folders have `Children`, files `Text` |
| `preview.Workspace.Changes` | `FileChange{Path, Status M/A/D, Added, Removed, Hunks}`; `Hunk{Header, Lines}`; `DiffLine{Kind, Old, New, Text}` |
| `preview.Workspace.Checkpoints` | `Checkpoint{ID, Turn, Time, Files, Added, Removed}`, newest first |
| `preview.Workspace.Documents` | `Document{ID, Title, Path, Text}` (markdown) |

### Layout

- Strip: 40 DIPs, tabs 28 high up to 180 wide with the icon and label; the
  active tab in `tab_active`; the close button shows on hover or on the
  active tab. It scrolls sideways; past `Overflow` (4) tabs a chevron
  (`navigation_tabs`) lists all with a check; "+" lists every resource
  with its shortcut. Menus are frosted (`kit.IconMenu`).
- Launcher: `details`, then 40 DIP secondary rows up to 440 wide: icon,
  label, Kbd shortcut. Order: Browser ⌘T, Terminal ⌃`, Files ⌘P, Git ⌃⇧G,
  History, Docs.
- Files: the shown file beside the 220 DIP tree (`file_tree`; rows 28, 16
  per level). Toolbar: Save, Undo, Redo (disabled, read-only), Find, and
  Document | Source for markdown (rendered with MyGo's markdown). Text with
  line numbers in mono 12. Status bar: path (right-click: Copy path),
  `Ln 1, Col 1`, UTF-8, language.
- Changes: hunks beside the 240 DIP list (`changes`, Stage all disabled
  with `git_actions_preview`; rows: file icon, name, status letter in
  warning/success/danger). A card per hunk: the `@@` line with Reject,
  Stage and Accept; lines with old and new numbers, added and removed in
  success and danger at 12%. Commit box: message, Commit (disabled),
  `git_preview_only`. Status bar: path, `+N −M`.
- History: a card per checkpoint: the turn, its age, the files, `+N −M`
  and `turn_changes_undo_all`.
- Documents: a row of document buttons over the rendered document.
- Browser: Back, Forward (disabled), Reload, the address
  (`browser_address`) and New tab over `resource_browser_preview`.
- Terminal: its title and New terminal over `resource_terminal_preview`.

## Actions

| Action | Effect in the preview |
|---|---|
| Open, choose and close tabs | `State` |
| Choose a file or a change | Shown |
| Accept a hunk | Marked accepted, toast `hunks_accepted` (local only, as the reference) |
| Reject, Stage, Undo changes, Find, Reload, address, New tab, New terminal | `preview_notice` toast: they write files, git, load pages or start a shell |
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
| Tabs, active tab, shown file and change, open folders, Document/Source, accepted hunks (until applied), commit draft, address | Files, changes, hunks, checkpoints, documents |

## Tests

`internal/panel/model_test.go`: tabs open once, closing picks the next.
`internal/shell/frame_test.go`: ⌘⌥B opens the launcher, ⌘P the files,
⌃⇧G the diff, Accept marks a hunk, six tabs show the overflow menu, ⌘W
closes tabs then the panel.

## Deviations

| Deviation | Reason |
|---|---|
| Resources are tabs in one panel | The plan asks for tabs; the reference holds one resource and tabs only a resource's documents |
| Checkpoints and documents are tabs | The reference restores checkpoints from a turn and draws documents in a plugin |
| Files and Git show one file at a time, without document tabs | The panel's own tabs carry the overflow |
| Hunks show in the Git tab, not a dialog | One place for the preview's diffs |
| Browser and terminal are placeholders in their chrome | Rendering is later work (MyGo WebView, libghostty-vt) |
