# Sidebar

The navigation column beside the rail: the new-conversation button, then
the pinned conversations, the chats without a project, and the projects
with their conversations as a tree
(the reference implementation `sidebar.rs`, `sidebar/*.rs`, `workspace/menus.rs`,
`workspace/project_editor*.rs`, `workspace/project_removal.rs`,
`ui/session_preview.rs`). The column's width, collapse and motion belong
to the frame (`docs/shell.md`).

## Package and files

`internal/desktop/sidebar`

| File | Holds |
|---|---|
| `sidebar.go` | `State`, `View`, groups, project rows, session rows, the pinned reorder, the hover card, `Age`, `Title` |
| `order.go` | **The rules** as pure functions: `Window`, `OrderPins`, `NextPinOrder`, `MovePin`, `Sort`, `Sorted`, `SortOf`, `Adjacent`, `ValidTitle` |
| `menus.go` | The right-click menus, archive and removal confirmations, the rename input |
| `editor.go` | The project editor and Open folder: `Editor`, `SuggestName`, `CheckProject`, `CheckClone`, `Destination`, `OpenFolder` |
| `metrics.go` | The optional live CPU and memory footer |

## Entry point

```go
s := sidebar.New(data, selectedID)
sidebar.View(c, s)          // in the navigation column
s.NewChat()                 // ⌘N, the button and the chats' "+"
s.TakeDraft()               // New session was asked for: focus the composer
s.TakeShown()               // the sidebar opened a conversation: go to it
s.OpenEditor(nil)           // ⌘⌥N and the projects' "+"; a project to edit it
s.Order()                   // the order of ⌘⇧[ and ⌘⇧]: the rows', top to bottom
s.Sort                      // *int, a Sort: the shell points it at the session order setting
s.Collapsed(), s.SetCollapsed(ids) // the projects closed, kept by the shell
```

## View model

The sidebar reads `preview.Data` (the boundary for a backend):

| Field | Meaning |
|---|---|
| `Conversation{ID, Title, Status, Updated}` | A row: its title (`conversation` while empty), status in the elbow slot, age from `Updated` |
| `Conversation.Pinned`, `PinOrder` | In the pinned group; `PinOrder` nil for a pin from before pins had an order (`preferences/pins.rs`) |
| `Conversation.Archived` | Hidden from every group |
| `Conversation.Created` | Creation order, newest highest: a tie between pins without an order |
| `Conversation.Updated` | The last run, zero for one that never ran: the age, and the Recently updated order |
| `Conversation.Worktree` | The branch of its worktree when not the main one: a `Network` icon on the row |
| `Project{ID, Name, Path, Branch, Host, Worktrees, Trusted}` | A project row; the hover card's branch and path; the worktree submenu |
| `Data.Hosts` | The editor's host select |

### Groups

| Group | Contents | Limit | Open at start |
|---|---|---|---|
| `sidebar_pinned` | `OrderPins` of pinned, not archived; hidden when empty | `Window` (5, Show more) | yes |
| `sessions_unassigned` | Chats without a project; New chat (`Plus`, `workspace_new_session`) | `Window` | yes |
| `projects` | Each project, a collapsible row with its conversations on the tree guide; Sort (`ListFilter`, `sidebar_sort`) and Add project (`FolderPlus`, `project_add`) | `Window` per project | yes, per project |

There is no Recent group: every session is already listed in its own group.
An empty group, or a project without sessions, is its header alone, with no
placeholder line.

Groups and projects grow open and closed through MyGo's collapsible (its
motion; at once with Reduce Motion). The chevron shows on hover. The header
actions always show, at the header's right: `kit.IconButton` 20 square, 8
apart and 8 from the edge, as a row's Pin and Archive, with the same hover.

### Sort

The projects' Sort button opens a Kit menu (`kit.MenuButton`) of three
orders with a check on the current one. The choice is the `session_order`
preference, shared with Settings › Conversation › Session order, and orders
the projects, the chats, and the sessions inside each project (`Sorted`).

| Menu item | `session_order` | Sessions | Projects |
|---|---|---|---|
| `session_order_recent` (default) | `updated`, or none | Never run first, then by last run (finished, else started), newest first | By the latest run among their sessions, newest first; a project none of whose sessions ran comes first |
| `session_order_created` | `created` | The Node's order: newest created first | Newest added first |
| `session_order_name` | `name` | By title without case; untitled first | By name without case |

Every sort is stable over the Created order, which breaks ties. Names
compare by lowercased code point, without locale collation.

Pinned sessions stay first: the pinned group is above the others and is
never sorted by the choice. It keeps `OrderPins`, its own manual order. A
pinned session's own row inside its project or the chats follows the chosen
order as any other row. The Node's durable manual order (`set_session_order`)
is part of the Node's order, so only Created shows it; the other two orders
use it for ties only. A project's row therefore drags before or after
another of its project under Created alone; the reference disables the drag
under `updated`.

### Rows

28 DIPs, text 14, indent 30, the dashed guide at 16 under projects, the
status slot (`kit.LoaderIn` 12 in the row's text color while running, the
waiting or unread dot) on its elbow. Hover shows Pin or
Unpin (`PinOff`) and Archive (`Inbox`); otherwise the age, and in pinned
rows the project's name. Running rows show no age. The pointer resting on
a row shows its hover card (`kit.SessionCard`, `sidebar/details.rs`) right
of the row: the title, the project and branch with their icons, and the
mono worktree path.

## Actions

| Action | Where | Effect in the preview |
|---|---|---|
| Open | click | `Selected`; beside a host or project page the click goes back to the conversation, also on the selected row (`TakeShown`) |
| New session | the top button, the chats' "+", ⌘N | `NewChat`: the unsent chat when there is one, otherwise a new empty chat. See [New session](#new-session) |
| Rename | double click, menu `workspace_rename` | In place: an input with the title; Enter or leaving keeps it, Escape gives up; `ValidTitle` failing toasts `session_title_required` and keeps the input |
| Pin / Unpin | hover button, menu | `TogglePin`: appended with `NextPinOrder`; unpin clears the order |
| Reorder pins | drag a pinned row | `MovePin`; a 2 DIP primary line shows before or after the target; every pin gets its index as order |
| Reorder a project's sessions | drag a project's row onto another of the project, under the Created order | `MoveSession`; the same 2 DIP line; the project's rows move in memory |
| Mark read / unread | menu | `Status` Unread ↔ Idle |
| Archive | hover button, menu | Confirm (`workspace_archive`, `workspace_memory_only`, `workspace_confirm`), then hidden in memory |
| Delete | menu | Confirm naming the session once (`session_delete`, `session_delete_named`), and for a chat without a project that its private workspace's files go with it (`session_delete_private_files`), then removed from memory; its split pane leaves with it |
| Export, Fork, Open file, New window | menu | `preview_notice` toast |
| Copy title, ID, path | menu › `content_copy` | Clipboard |
| New session | project menu | The project's unsent conversation when it has one, otherwise an empty one in the project, in memory |
| New terminal, Records, Overview, Worktrees, AI CLI | project menu | `preview_notice` toast |
| Edit project | project menu | The editor with its name, path and appearance |
| Trust (`project_trust`) | project menu, preview only | Confirm (`project_grant_title` or `project_revoke_title`, the name, the path, the description and `project_trust_preview`), then `Trusted` flips in memory. The reference's live menu has no such item |
| Remove project | project menu | A project with sessions is refused with the toast `project_remove_sessions`; otherwise confirm naming the path once (`project_remove_sample`, `project_remove_preview`), then removed from memory |
| Open folder | `folder.open` (the menu, the shortcut) | The directory picker titled `menu_open_folder`; the choice opens the project already at that path on the shown host, otherwise adds one named after the directory |

Right-click menus are the system's (`Element.ContextMenu`); their items and
order follow `live/menus/session_actions.rs` and `workspace/menus.rs`.

### New session

The sidebar shows beside the conversation and the host and project pages
(docs/shell.md, Side column per page), always with its New session button
(`kit.Primary`), as the reference's `new-conversation` button.

As the reference keeps a single draft (`new_live_conversation` activates
the `Key::Draft` panel when there is one), New session never stacks empty
chats: `NewChat` opens the unsent chat when there is one and makes a new
one otherwise. A chat is unsent while it has no title, no turns and is
idle. That rule is the preview's alone: live, the runtime's `NewDraft`
decides, and its drafts are not in the sidebar's data at all (`listSidebar`),
which is the one place that keeps them out of the tree.

| State | New session and the chats' "+" |
|---|---|
| Another session shows, or a host or project page | Enabled: opens the unsent chat, or a new one, on the conversation page |
| The conversation page already shows the unsent chat | Enabled, as the reference's: the chat stays as it is and its composer takes the focus again (`TakeDraft`, `composer.State.Focus`). ⌘N and the menu's item do the same |

### Project editor

`kit.Dialog` 560 wide, description `project_editor_preview` in the preview.
It follows `workspace/project_editor.rs` and `project_editor/form.rs`
element by element:

| Element | Behavior |
|---|---|
| Source (`kit.Segmented`, block) | `project_local` \| `project_clone`, for a new project only, in the preview and live. Each mode keeps its own path: switching swaps the path field with the other mode's and suggests the name again |
| Appearance (`kit.AppearancePicker`) | The 32 DIP button before the name shows the chosen glyph; its popover holds 11 colors (`project_color_*`: none, blue, indigo, violet, magenta, red, orange, amber, green, teal, cyan) and 30 icons (`project_icon_*`, `icons.Projects`, from the reference's reicon set) tinted with the chosen color. Default `folder` and `none`; an existing project opens on its own |
| `project_name` | Suggested by `SuggestName` from the path's last element, or the repository's without `.git`, until the user types a different name; an empty name is suggested again on save |
| `project_host` (`kit.Select`) | The hosts of `Data.Hosts`; disabled when editing and while saving. Changing it clears the path, since a path belongs to its host |
| `project_repository`, `project_branch` | Clone only; the branch is optional |
| `project_path` or `project_destination` | With the resulting `Destination` under it for a clone; the folder button (`directory_title`) opens the [directory picker](files.md#host-file-picker) on the selected host at the typed path. The answer is dropped if the host or the path changed meanwhile |
| Save | `settings_save` (`settings_save_preview` in the preview), `project_clone` for a clone. Disabled with the busy mark (`kit.Busy`) from the click to the Node's answer; every field is disabled meanwhile. The outcome is a toast and a failure keeps the draft |
| `project_path` or `project_destination` | With the resulting `Destination` under it for a clone; the folder button (`directory_title`) opens the [directory picker](files.md#host-file-picker) on the selected host at the typed path, its Projects group listing that host's registered projects (`projectPlaces`). The answer is dropped if the host or the path changed meanwhile |
| Save | `settings_save` (`settings_save_preview` in the preview), `project_clone` for a clone. Disabled with the busy mark (`kit.PrimaryBusy`, which draws `kit.Busy`) from the click to the Node's answer; every field is disabled meanwhile. The outcome is a toast and a failure keeps the draft |

The preview checks `CheckProject` (`project_required`,
`project_duplicate_path`) or `CheckClone` (`project_clone_required`). Live,
a clone is checked the same way and a local project is sent as typed, as
the reference does: the Node validates the name and the path.

The sidebar's project row draws the project's glyph (`kit.ProjectGlyph`):
the icon in the color's 600 shade on light and 400 on dark, `none` in the
muted foreground, an unknown icon as the folder.

## Backend ownership

The B3 live adapter, controller preferences and remaining phase boundaries are
specified in [B3 live wiring](#b3-live-wiring). The preview remains an explicit,
separate constructor and does not own backend facts. Later feature phases supply
their owning Client actions and projections without changing the accepted UI.

## UI state vs backend data

| UI state (`State`) | Backend data |
|---|---|
| `Selected`, group and project open flags (projects kept by the shell), Show more, the rename draft, the editor draft, the open confirmation | Conversations, projects, hosts, pins and their order, titles, archive; the sort order is a controller preference |

## Tests

`internal/desktop/sidebar/order_test.go`: legacy pins first, appended pin order,
`MovePin`, `Window`, `Sorted` in the three orders and the preference's values,
`Adjacent` wrapping, `ValidTitle`.
`internal/desktop/sidebar/editor_test.go`: name suggestion, duplicate paths per
host, clone names and destinations, and that only a repository loses `.git`.
`TestProjectEditorAndRemoval` in `internal/desktop/shell/sidebar_test.go`
drives the editor in the preview: validation, the appearance popover, the
picker over the sample directories (filter, Enter, Backspace, Back, the
entry menu, the hidden toggle, grid and list, the choice filling the path
and the name), Clone with its repository, branch and destination, the saved
appearance in the sidebar, editing, the project menu's items in order,
trust, Open folder adding and reopening, the refusal to remove a project
with sessions and the removal of one without. `firstRunProjects` in
`internal/desktop/live/first_run_test.go` does it against a real Node: the
picker lists a temporary directory tree through `browse_files`, Save is
pending while `create_project` is held, the appearance reaches
`create_project` and `update_project` and the sidebar, a second real Node
bound as a paired Host is browsed and receives its project through its own
controller, and removal names the consequence and toasts `project_removed`.
`internal/desktop/shell/sidebar_test.go` also covers:
the native menu pins and renames, an empty rename stays out and Escape
gives up, pins reorder, the editor validates and adds, removal names the
project and removes it; New session opens one unsent chat, stays enabled over
it and gives its editor the focus back, and leads back from a host page, in
the preview and with the live draft action (`testSidebarNewSession`); empty
groups are their headers alone, there is no Recent group, the header actions
show without the pointer at one size, the sort menu's three orders give the
expected projects and sessions with the pinned one first, the preview keeps
the order across a relaunch and an older `ui-state.json` loads
(`testSidebarGroups`). `internal/desktop/live/first_run_test.go` does the same
against a real Node. `internal/desktop/live/projection_test.go`: the
projection keeps the Node's order under every preference, the sidebar orders
it, and the choice survives a restart of the runtime through
`preferences.json`.
`internal/desktop/shell/shell_test.go`: Show more.
`internal/desktop/shell/loading_test.go` (`TestShellRules/LoadingMarks`): a
running row has the 12 DIP loader; the sidebar alone asks for frames while a
session runs, for none with Reduce Motion, and has no loader and asks for
none once no session runs.

## Deviations

| Deviation | Reason |
|---|---|
| Rename is in place | The plan asks for inline rename; the reference renames in a dialog |
| Pinned rows reorder by dragging | The plan asks for it; the reference orders pins by when they were pinned |
| No Recent group; empty groups show no placeholder | The user's decision: Recent repeats sessions listed in their groups, and an empty group is its header |
| Header actions always show; the projects' header has a Sort menu | The user's decision, after Cursor's repository header; the reference shows "+" on hover and sorts only from Settings |
| `session_order` gains `name`, defaults to `updated`, and also orders projects | The user's decision. The reference has `created` (its default) and `updated`, for sessions only. A profile without a value now reads as `updated`; a stored `created` is kept |
| Project sessions reorder by dragging only under the Created order; no split folders | Name and Updated do not show the manual order, so a move there would not show |
| The pinned group does not append the host's running sessions | The preview has one host's pins only |
| Chats keep the 5-row limit | Phase 1's accepted behavior; the reference scrolls a 7.4 rem box |
| No terminals in the tree | Terminals come with the terminal work |
| The metrics footer is off by default | The existing controller preference enables B7's live CPU and memory display |
| The live project menu keeps New terminal, Records, Worktrees and AI CLI | They were accepted with the page; the reference's live menu has Overview, Edit, New session, Copy path and Remove, then the items packages contribute. Contributed items follow Records here; the bundled ones these four replace are not repeated ([plugins](plugins.md#deviations)) |
| The running row's loader is MyGo's `loading.Swirl` in one color | The user's decision; the dot matrix in the info, warning and danger colors is removed ([components.md](components.md#deviations)) |
| The appearance popover lays the icons out six to a row | `kit.Popover` with Kit ghost buttons; the reference's popover wraps them freely |

## B3 live wiring

Sidebar.State consumes neutral `viewmodel.Data`; preview type aliases keep the
accepted preview behavior. Optional sidebar.Actions routes live mutations to the
Client. Names, ownership, session/attention revisions and project appearance are
captured in the presentation records. Inline rename retains the record captured
when editing started; project edits retain the complete expected Project.
Pending saves disable controls, keep their drafts on failure/conflict/unknown,
and ignore a completion after the editor has opened a different draft.

`live.Sidebar` projects visible projects, worktrees and sessions in the Node's
creation/manual authority order, whatever the preference; the sidebar orders
them when it draws ([Sort](#sort)). The sort menu and the Conversation setting
edit the same `settings.Model.SessionOrder`, which the runtime stores as
`session_order` in `desktop/preferences.json`; the preview keeps it as
`sessionOrder` in `ui-state.json`. Shared Client activity lanes supply running, waiting and
finished-unread status. SessionsRemoved replaces presentation data and prunes
panes; project removal hides its sessions without changing project files/history.

Rename/archive/read/remove dispatch the corresponding v1 commands. Session read
uses AttentionRevision; rename/archive/remove use Revision. Project creation is
local only in B3, and UpdateProject/RemoveProject carry the complete captured
expected Project. Directory selection is the in-app host file picker
(`Actions.Files`, docs/files.md), never a native dialog, as in the reference.
`create_project` and `update_project` carry the editor's appearance; a clone
sends `Destination` as its path with the trimmed URL and optional branch. A
failed save toasts `project_outcome_unknown` (unknown or unavailable),
`project_changed` (revision conflict), `project_path_in_use` (busy),
`project_clone_failed` or `live_request_failed`; a saved project is revealed
and its overview opens once the snapshot holds it. Removal confirms with
`project_remove_named`, toasts `project_removed`, and on failure toasts what
the Node reports (`project_remove_unknown`, `project_remove_tasks`,
`project_remove_tasks_partial`, `project_remove_changed_partial`,
`project_remove_failed`, `project_changed`) and asks again.
Directory open uses the system opener; new-window navigation shares the Node and
Client through the application window factory.

Pins are controller preferences keyed by lowercase NodeID/session ID. Unordered
legacy pins precede ordered ones. Large u64 orders map to presentation ranks
without integer overflow, while persisted values remain unchanged. Pin reordering
never dispatches SetSessionOrder; a project row's drag does. Archived/removed resources lose their current
Node's pins. Empty drafts remain UI-local and create no persisted session until
B4's first-send path. As the reference's draft is a panel and never a tree
row, live keeps drafts in the shell's data and gives the sidebar a copy
without them (`listSidebar`), so a draft has no row in its group.
New session reopens the location's unsent draft; one left unselected with no
text, file or reference is dropped after the frame. Unsupported later actions
remain unavailable.

## B7 live wiring

Combined presentation preserves each conversation's Node and each project's
Host identity while every shared Client retains its own snapshot. Actions
capture the owning Client instead of following later navigation. New projects
opened from a Host page default to that exact Host; an existing project's Host
remains read-only.

Project Overview opens `shell.ProjectPage` with the project's captured Node and
project ID. The page resolves current facts by both identities, independently of
the selected conversation, and stays unavailable if that owner disappears. New
session keeps the captured project. Records include ordinary project sessions,
including archived ones, exclude delegated and resource-owned sessions, and
search titles and worktree paths without changing ownership. Opening an archived
record uses its owning Client's revision-checked `SetSessionArchived` before
navigating; failure keeps the page open. A record's right-click menu is the
session's (`SessionMenu`); an archived record's has Unarchive
(`backup_unarchive`, `set_session_archived` at its revision), Delete and
Copy alone, as it has no row to rename or pin. The header's More menu
(`ProjectActions`) on the page has Edit, New worktree, Copy path, Remove
worktree, Remove project and Launch CLI, the sidebar's project actions; the
reference's Manage worktrees entry is the sidebar menu's submenu here.

Dragging a project's row sends `set_session_order` with every session of
the project in the snapshot's order as `expected` (delegated and archived
ones included, which the Node counts) and reports a conflict as
`session_changed`, an unknown outcome as `project_outcome_unknown` and any
other failure as `live_request_failed`, as the reference.

Worktree menu selection captures that project's Node and worktree, registers a
native checkout when needed, then opens the project overview without creating a
session draft. The selected worktree supplies the overview's Git status and
subsequent New session action; ordinary Overview defaults to the main worktree.
Failed or uncertain registration keeps the current navigation, and a late
registration result cannot replace a different page or conversation.

The overview reads Git status for its captured worktree. Directory
initialization uses the existing Node `RunGitAction` command and its expected
Git state; nested repositories require the ordinary confirmation. New terminal
and terminal records remain unavailable until B10. B8 connects the AI CLI
menu/overview callbacks to the owning Node's tool inventory and a captured
worktree launcher. Its minimal authorized terminal prerequisite opens the actual
Node-owned AI CLI through MyGo Mirror/Conn in the panel. General terminal
resources and split actions remain B10. The preview's project-menu behavior
remains unchanged.

The existing `SidebarMetrics` controller preference enables a CPU and memory
footer below the session tree. `sidebar.State.Metrics` carries the current
sample's optional percentages; missing data displays an em dash. The footer
uses the selected conversation's execution Node, or the selected Host while its
page is open. It requests summary samples only while the navigation is visible
and the preference is enabled. The Host details panel requests detailed samples
through the same Node sampler. Hiding both consumers cancels their current read
and stops polling; an empty footer never starts another Node or agent engine.
