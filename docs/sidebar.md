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
| `editor.go` | The project editor: `Editor`, `SuggestName`, `CheckProject`, `CheckClone`, `Destination` |
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
never sorted by the choice. It keeps `OrderPins`, which is the only manual
order: dragging reorders pinned rows only. A pinned session's own row inside
its project or the chats follows the chosen order as any other row. The
Node's durable manual order (`set_session_order`, which this desktop never
sends) is part of the Node's order, so only Created shows one that another
controller saved; the other two orders use it for ties only, as the
reference disables the drag under `updated`.

### Rows

28 DIPs, text 14, indent 30, the dashed guide at 16 under projects, the
status slot (loader, waiting, unread dot) on its elbow. Hover shows Pin or
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
| Mark read / unread | menu | `Status` Unread ↔ Idle |
| Archive | hover button, menu | Confirm (`workspace_archive`, `workspace_memory_only`, `workspace_confirm`), then hidden in memory |
| Delete | menu | Confirm naming the session once (`session_delete`, `session_delete_named`), then removed from memory; its split pane leaves with it |
| Export, Fork, Open file, New window | menu | `preview_notice` toast |
| Copy title, ID, path | menu › `content_copy` | Clipboard |
| New session | project menu | The project's unsent conversation when it has one, otherwise an empty one in the project, in memory |
| New terminal, Records, Overview, Worktrees, AI CLI | project menu | `preview_notice` toast |
| Edit project | project menu | The editor with its name and path |
| Remove project | project menu | Confirm naming the path once (`project_remove_named`, `project_remove_preview`), then removed from memory |

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

`kit.Dialog` 560 wide, description `project_editor_preview`. New
projects: `project_local` | `project_clone`. Fields: `project_name`
(suggested from the path or repository by `SuggestName` until typed over),
`project_host` (`kit.Select`; read-only when editing), for a clone
`project_repository` and `project_branch`, then `project_path` or
`project_destination` with the resulting `Destination` under it, and the
folder button (`directory_title`, preview toast). Save preview checks
`CheckProject` (`project_required`, `project_duplicate_path`) or
`CheckClone` (`project_clone_required`) and toasts the failure, keeping
the draft.

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
host, clone names and destinations. `internal/desktop/shell/sidebar_test.go`:
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

## Deviations

| Deviation | Reason |
|---|---|
| Rename is in place | The plan asks for inline rename; the reference renames in a dialog |
| Pinned rows reorder by dragging | The plan asks for it; the reference orders pins by when they were pinned |
| No Recent group; empty groups show no placeholder | The user's decision: Recent repeats sessions listed in their groups, and an empty group is its header |
| Header actions always show; the projects' header has a Sort menu | The user's decision, after Cursor's repository header; the reference shows "+" on hover and sorts only from Settings |
| `session_order` gains `name`, defaults to `updated`, and also orders projects | The user's decision. The reference has `created` (its default) and `updated`, for sessions only. A profile without a value now reads as `updated`; a stored `created` is kept |
| Project sessions do not reorder by dragging; no split folders | Not in this phase |
| The pinned group does not append the host's running sessions | The preview has one host's pins only |
| Chats keep the 5-row limit | Phase 1's accepted behavior; the reference scrolls a 7.4 rem box |
| No terminals in the tree | Terminals come with the terminal work |
| The metrics footer is off by default | The existing controller preference enables B7's live CPU and memory display |

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
expected Project. Directory selection is supplied by the native window adapter.
Directory open uses the system opener; new-window navigation shares the Node and
Client through the application window factory.

Pins are controller preferences keyed by lowercase NodeID/session ID. Unordered
legacy pins precede ordered ones. Large u64 orders map to presentation ranks
without integer overflow, while persisted values remain unchanged. Pin reordering
never dispatches SetSessionOrder. Archived/removed resources lose their current
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
navigating; failure keeps the page open.

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
