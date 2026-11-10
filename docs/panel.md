# Right panel

The column right of the conversation: one flat strip of tabs, each an
open item, over the active one, or the launcher when none is open (the reference implementation
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
panel.View(c, s, pr, shell.KeyText)   // the tab host; KeyText writes the launcher's shortcuts
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
  folder it starts in (the selected project's path, muted), Find, Close
  (`workspace_close_terminal`) and New terminal, over
  `workspace_terminal_preview`. The live terminal has no Reset.
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
worktree. Sequential batches stop on the first failure and retain their confirmed
position for retry. Native open, reveal, download and reference insertion use
`WorkspaceNative`; execution-host paths never become controller-local paths.
The window's transfer owner verifies downloaded content and owns native effects.

Git status and branches project into the existing fields. Existing menus send
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
and stops at the first failure. Session documents read `ListArtifacts` and
`ReadArtifact`, converting stored HTML to Markdown outside the UI thread. A
revision cache avoids repeated reads on each presentation frame. Outcomes appear
only in distinct failure, conflict, uncertain or success toasts.

Browser and terminal rendering belongs to the later resource phase.

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

## UI state vs backend data

| UI state (`State`) | Backend data |
|---|---|
| Tabs, active tab; per browser tab the address and history, per terminal tab the folder and find query; per project: shown file and change, open folders, Document/Source, scroll, accepted hunks (until applied), commit draft | Files, changes, hunks, checkpoints, documents, registered child transcripts |

## Tests

`internal/desktop/live/workspace_projection_test.go` covers status and nested file references, unique hunk identity and commit scopes. `internal/desktop/panel/live_test.go` covers draft preservation across refresh and in-flight saves, exact file endings and native hunk selection.

`internal/desktop/panel/editor_test.go` groups exact source restoration, lossless
fallback selection, formatting boundaries, overflow ordering and close lifecycle
checks. Notebook draft validation, output projection and bounded embedded images
join the attachment projection group in `internal/desktop/live`.

`internal/desktop/panel/model_test.go`: tabs open once, closing picks the next,
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
the terminal has no Reset and ⌘F in it opens its find bar, ⌘W closes tabs
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
| Every open item is a tab in one flat strip | The plan asks for tabs; the reference holds one resource and tabs only a resource's documents |
| Checkpoints and documents are tabs | The reference restores checkpoints from a turn and draws documents in a plugin |
| Files and Git keep their state per project | The reference keeps one and resets it when the project changes |
| Each open file and diff is a tab, also on the pages' strips | The plan asks for several files and diffs open at once |
| The split's list is a fixed column past our own drag handle | MyGo's `Split` sizes its first pane, so a first frame jumped |
| Hunks show in the Git tab's main area, not a dialog | One place for the preview's diffs |
| Preview Git forms, Output, commit and stash documents show the notice | Live adapters require a real repository; B0 remains isolated |
| Browser and terminal are placeholders in their chrome | Rendering is later work (MyGo WebView, libghostty-vt) |
| The browser's chrome is core | In the reference the browser plugin draws it; the preview has no plugin UI host |

## Native workspace activity

`workspace_activity.go` fills the existing composer Git capsule from the same
captured `InspectGit` state as the panel. Totals include every returned entry;
the recent list uses modification time descending, stable path order for ties,
and the existing ten-file limit. Its action opens the actual worktree diff.
Final turn diffs populate conversation changes cards. Review captures that turn's
patches and hunk identities in the existing Git surface; Undo confirms the target
and restores matching checkpoints in source order. A partially confirmed restore
resumes after its successful entries and retains uncertain request identity.

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
