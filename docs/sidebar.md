# Sidebar

The navigation column beside the rail: the new-conversation button, then
the pinned conversations, the chats without a project, the projects with
their conversations as a tree, and the recent conversations
(sailry-public `sidebar.rs`, `sidebar/*.rs`, `workspace/menus.rs`,
`workspace/project_editor*.rs`, `workspace/project_removal.rs`,
`ui/session_preview.rs`). The column's width, collapse and motion belong
to the frame (`docs/shell.md`).

## Package and files

`internal/sidebar`

| File | Holds |
|---|---|
| `sidebar.go` | `State`, `View`, groups, project rows, session rows, the pinned reorder, the hover card, `Age`, `Title` |
| `order.go` | **The rules** as pure functions: `Window`, `OrderPins`, `NextPinOrder`, `MovePin`, `RecentOrder`, `Adjacent`, `ValidTitle` |
| `menus.go` | The right-click menus, archive and removal confirmations, the rename input |
| `editor.go` | The project editor: `Editor`, `SuggestName`, `CheckProject`, `CheckClone`, `Destination` |

## Entry point

```go
s := sidebar.New(data, selectedID)
sidebar.View(c, s)          // in the navigation column
s.NewChat()                 // ⌘N, the button and the chats' "+"
s.OpenEditor(nil)           // ⌘⌥N and the projects' "+"; a project to edit it
s.Order()                   // the order of ⌘⇧[ and ⌘⇧]
s.Collapsed(), s.SetCollapsed(ids) // the projects closed, kept by the shell
```

## View model

The sidebar reads `preview.Data` (the boundary for a backend):

| Field | Meaning |
|---|---|
| `Conversation{ID, Title, Status, Updated}` | A row: its title (`conversation` while empty), status in the elbow slot, age from `Updated` |
| `Conversation.Pinned`, `PinOrder` | In the pinned group; `PinOrder` nil for a pin from before pins had an order (`preferences/pins.rs`) |
| `Conversation.Archived` | Hidden from every group |
| `Conversation.Created` | Creation order, newest highest: the recent group's order |
| `Conversation.Worktree` | The branch of its worktree when not the main one: a `Network` icon on the row |
| `Project{ID, Name, Path, Branch, Host, Worktrees, Trusted}` | A project row; the hover card's branch and path; the worktree submenu |
| `Data.Hosts` | The editor's host select |

### Groups

| Group | Contents | Limit | Open at start |
|---|---|---|---|
| `sidebar_pinned` | `OrderPins` of pinned, not archived; hidden when empty | `Window` (5, Show more) | yes |
| `sessions_unassigned` | Chats without a project; "+" `workspace_new_session` | `Window` | yes |
| `projects` | Each project, a collapsible row with its conversations on the tree guide; "+" `project_add` | `Window` per project | yes, per project |
| `recent_sessions` | `RecentOrder` of all conversations, no status | none | no |

Groups and projects grow open and closed through MyGo's collapsible (its
motion; at once with Reduce Motion). The chevron and "+" show on hover.

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
| Open | click | `Selected` |
| Rename | double click, menu `workspace_rename` | In place: an input with the title; Enter or leaving keeps it, Escape gives up; `ValidTitle` failing toasts `session_title_required` and keeps the input |
| Pin / Unpin | hover button, menu | `TogglePin`: appended with `NextPinOrder`; unpin clears the order |
| Reorder pins | drag a pinned row | `MovePin`; a 2 DIP primary line shows before or after the target; every pin gets its index as order |
| Mark read / unread | menu | `Status` Unread ↔ Idle |
| Archive | hover button, menu | Confirm (`workspace_archive`, `workspace_memory_only`, `workspace_confirm`), then hidden in memory |
| Delete, Export, Fork, Open file, New window | menu | `preview_notice` toast |
| Copy title, ID, path | menu › `content_copy` | Clipboard |
| New session | project menu | An empty conversation in the project, in memory |
| New terminal, Records, Overview, Trust, Worktrees, AI CLI | project menu | `preview_notice` toast |
| Edit project | project menu | The editor with its name and path |
| Remove project | project menu | Confirm naming the path once (`project_remove_named`, `project_remove_preview`), then removed from memory |

Right-click menus are the system's (`Element.ContextMenu`); their items and
order follow `live/menus/session_actions.rs` and `workspace/menus.rs`.

### Project editor

`kit.Dialog` 560 wide, description `project_editor_preview`. New
projects: `project_local` | `project_clone`. Fields: `project_name`
(suggested from the path or repository by `SuggestName` until typed over),
`project_host` (`ui.Select`; read-only when editing), for a clone
`project_repository` and `project_branch`, then `project_path` or
`project_destination` with the resulting `Destination` under it, and the
folder button (`directory_title`, preview toast). Save preview checks
`CheckProject` (`project_required`, `project_duplicate_path`) or
`CheckClone` (`project_clone_required`) and toasts the failure, keeping
the draft.

## Wiring a backend

Fill `preview.Data`'s projects and conversations from the snapshot (or
replace the type with the backend's, keeping the fields above). Persist
pins (`Pinned`, `PinOrder`) in the preferences as `pins.rs` does. The
actions that show `preview_notice` become requests: rename and archive
send their commands with the expected revision and report outcomes as
toasts only; the editor's save sends Create/UpdateProject.

## UI state vs backend data

| UI state (`State`) | Backend data |
|---|---|
| `Selected`, group and project open flags (projects kept by the shell), Show more, the rename draft, the editor draft, the open confirmation | Conversations, projects, hosts, pins and their order, titles, archive |

## Tests

`internal/sidebar/order_test.go`: legacy pins first, appended pin order,
`MovePin`, `Window`, `RecentOrder`, `Adjacent` wrapping, `ValidTitle`.
`internal/sidebar/editor_test.go`: name suggestion, duplicate paths per
host, clone names and destinations. `internal/shell/sidebar_test.go`:
the native menu pins and renames, an empty rename stays out and Escape
gives up, pins reorder, the editor validates and adds, removal names the
project and removes it. `internal/shell/shell_test.go`: Show more.

## Deviations

| Deviation | Reason |
|---|---|
| Rename is in place | The plan asks for inline rename; the reference renames in a dialog |
| Pinned rows reorder by dragging | The plan asks for it; the reference orders pins by when they were pinned |
| Project sessions do not reorder by dragging; no split folders | Not in this phase |
| The pinned group does not append the host's running sessions | The preview has one host's pins only |
| Chats keep the 5-row limit | Phase 1's accepted behavior; the reference scrolls a 7.4 rem box |
| No terminals in the tree; the sidebar metrics footer is not shown | Terminals come with the terminal work; metrics are off by default |
