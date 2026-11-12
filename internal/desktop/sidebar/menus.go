package sidebar

import (
	"fmt"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// notice tells that an action would leave the preview: it would write
// files, run git, a terminal or a model.
func notice(c *ui.Context) { kit.PreviewNotice(c) }

func (s *State) notice(c *ui.Context) {
	if s.Actions != nil {
		kit.Notify(c, "", i18n.T(c, "unavailable"))
	} else {
		notice(c)
	}
}

// sessionMenu is a session row's right-click menu, the system's, with the
// reference's items in its order. Actions on the preview's rows change them
// in memory; the rest tell the preview notice. The demo, which says nothing
// of itself, forks in memory and leaves out what needs a Node or the system:
// Export, Open folder and New window.
func (s *State) sessionMenu(c *ui.Context, m *ui.Menu, cv *preview.Conversation) {
	t := func(key string) string { return i18n.T(c, key) }
	if cv.Archived {
		s.archivedMenu(c, m, cv)
		return
	}
	if m.Item(t("workspace_rename")).Chosen() {
		s.StartRename(cv)
	}
	pin := t("workspace_pin")
	if cv.Pinned {
		pin = t("workspace_unpin")
	}
	if m.Item(pin).Chosen() {
		s.TogglePin(cv)
	}
	read := t("session_mark_unread")
	if cv.Status == preview.Unread {
		read = t("session_mark_read")
	}
	if m.Item(read).Disabled(cv.Status == preview.Running || cv.Status == preview.Waiting).Chosen() {
		if s.Actions != nil && s.Actions.Read != nil {
			s.Actions.Read(cv, cv.Status == preview.Unread)
		} else if cv.Status == preview.Unread {
			cv.Status = preview.Idle
		} else {
			cv.Status = preview.Unread
		}
	}
	m.Separator()
	if m.Item(t("workspace_archive")).Chosen() {
		s.archive(c, cv)
	}
	if m.Item(t("session_delete")).Chosen() {
		// A session without a project keeps its files in a private
		// workspace, which goes with it: the confirmation says so once.
		message := i18n.T(c, "session_delete_named", "name", Title(c, cv))
		if s.Data.ProjectOf(cv.ID) == nil {
			message += "\n" + t("session_delete_private_files")
		}
		s.confirm = confirm{open: true, title: t("session_delete"), message: message,
			action: t("session_delete"), run: func() { s.delete(cv) }}
	}
	m.Separator()
	pr := s.Data.ProjectOf(cv.ID)
	m.Submenu(t("content_copy"), func(m *ui.Menu) {
		if m.Item(t("workspace_copy_title")).Chosen() {
			c.WriteClipboard(Title(c, cv))
		}
		if m.Item(t("session_copy_id")).Chosen() {
			c.WriteClipboard(cv.ID)
		}
		if m.Item(t("workspace_copy_path")).Disabled(pr == nil).Chosen() {
			c.WriteClipboard(sessionPath(pr, cv))
		}
	})
	// The demo's sessions have no file to write.
	demo := s.Actions == nil && kit.Demo(c)
	if !demo {
		m.Submenu(t("conversation_export"), func(m *ui.Menu) {
			for format, key := range []string{"conversation_export_markdown", "conversation_export_html", "conversation_export_image"} {
				if m.Item(t(key)).Disabled(!conversation.ExportSupported(conversation.ExportFormat(format))).Chosen() {
					if s.Actions != nil && s.Actions.Export != nil {
						s.Actions.Export(cv, conversation.ExportFormat(format))
					} else {
						s.notice(c)
					}
				}
			}
		})
	}
	m.Separator()
	busy := cv.Status == preview.Running || cv.Status == preview.Waiting
	m.Submenu(t("session_fork"), func(m *ui.Menu) {
		if m.Item(t("session_fork_here")).Disabled(busy || pr == nil).Chosen() {
			if s.Actions != nil && s.Actions.ForkHere != nil {
				s.Actions.ForkHere(cv)
			} else if demo {
				s.fork(pr, cv, false)
			} else {
				s.notice(c)
			}
		}
		if m.Item(t("session_fork_worktree")).Disabled(busy || pr == nil).Chosen() {
			if s.Actions != nil && s.Actions.ForkWorktree != nil {
				s.Actions.ForkWorktree(cv)
			} else if demo {
				s.fork(pr, cv, true)
			} else {
				s.notice(c)
			}
		}
	})
	if demo {
		// Its folders are not on this disk, and it opens no second window.
		return
	}
	m.Separator()
	if m.Item(t("files_open")).Disabled(pr == nil).Chosen() {
		if s.Actions != nil && s.Actions.OpenWorkspace != nil {
			s.Actions.OpenWorkspace(pr, cv)
		} else {
			s.notice(c)
		}
	}
	if m.Item(t("session_new_window")).Chosen() {
		if s.Actions != nil && s.Actions.OpenWindow != nil {
			s.Actions.OpenWindow(cv.ID)
		} else {
			s.notice(c)
		}
	}
}

// SessionMenu is the right-click menu of a session's row outside the
// sidebar, as a project page's record.
func (s *State) SessionMenu(c *ui.Context, m *ui.Menu, cv *preview.Conversation) {
	s.sessionMenu(c, m, cv)
}

// archivedMenu is an archived session's menu: it has no row to rename or
// pin, so it restores, deletes or copies.
func (s *State) archivedMenu(c *ui.Context, m *ui.Menu, cv *preview.Conversation) {
	t := func(key string) string { return i18n.T(c, key) }
	if m.Item(t("backup_unarchive")).Chosen() {
		if s.Actions != nil && s.Actions.Unarchive != nil {
			s.Actions.Unarchive(cv)
		} else if s.Actions == nil {
			cv.Archived = false
		}
	}
	if m.Item(t("session_delete")).Chosen() {
		s.confirm = confirm{open: true, title: t("session_delete"), message: i18n.T(c, "session_delete_named", "name", Title(c, cv)),
			action: t("session_delete"), run: func() { s.delete(cv) }}
	}
	m.Separator()
	m.Submenu(t("content_copy"), func(m *ui.Menu) {
		if m.Item(t("workspace_copy_title")).Chosen() {
			c.WriteClipboard(Title(c, cv))
		}
		if m.Item(t("session_copy_id")).Chosen() {
			c.WriteClipboard(cv.ID)
		}
	})
}

// ProjectActions fills the project page's More menu with the project's
// actions, as the reference's header menu.
func (s *State) ProjectActions(c *ui.Context, m *kit.Menu, pr *preview.Project) {
	t := func(key string) string { return i18n.T(c, key) }
	worktrees := func(action string) { s.worktrees(c, pr, action, "") }
	if m.Item(t("project_edit")).Icon(icons.Settings2).Chosen() {
		s.OpenEditor(pr)
	}
	if m.Item(t("worktree_create")).Icon(icons.Plus).Chosen() {
		worktrees("create")
	}
	if m.Item(t("workspace_copy_path")).Icon(icons.Copy).Chosen() {
		c.WriteClipboard(pr.Path)
	}
	if m.Item(t("worktree_remove")).Icon(icons.CircleX).Disabled(len(pr.Worktrees) < 2).Chosen() {
		worktrees("remove")
	}
	if m.Item(t("project_remove")).Icon(icons.Trash).Danger().Chosen() {
		s.AskRemove(c, pr)
	}
}

// projectMenu is a project row's right-click menu, the system's, with the
// reference's groups in its order: the project's page and editor, what is
// made in it, its path, its removal. The reference's own items are Project
// overview, Edit project, New session, Copy path and Remove project; what
// follows New session is the project_menu slot's, the enabled packages'
// entries. The live window fills the slot through Actions.ProjectMenu; a
// sample dataset has the bundled packages' two, New terminal and the
// worktrees, which act on it in memory.
func (s *State) projectMenu(c *ui.Context, m *ui.Menu, pr *preview.Project) {
	t := func(key string) string { return i18n.T(c, key) }
	if m.Item(t("workspace_project")).Chosen() {
		s.OpenOverview(c, pr)
	}
	if m.Item(t("project_edit")).Chosen() {
		s.OpenEditor(pr)
	}
	m.Separator()
	if m.Item(t("workspace_new_session")).Chosen() {
		s.newIn(pr)
	}
	if s.Actions == nil {
		s.TerminalItem(c, m, pr)
		s.WorktreeMenu(c, m, pr)
	} else if s.Actions.ProjectMenu != nil {
		s.Actions.ProjectMenu(c, m, pr)
	}
	m.Separator()
	if m.Item(t("workspace_copy_path")).Chosen() {
		c.WriteClipboard(pr.Path)
	}
	m.Separator()
	if m.Item(t("project_remove")).Chosen() {
		s.AskRemove(c, pr)
	}
}

// OpenOverview shows a project's page: the live adapter's, or the sample
// dataset's through the window.
func (s *State) OpenOverview(c *ui.Context, pr *preview.Project) {
	switch {
	case s.Actions != nil && s.Actions.Overview != nil:
		s.Actions.Overview(pr)
	case s.Actions == nil && s.Sample.Overview != nil:
		s.Sample.Overview(pr)
	default:
		s.notice(c)
	}
}

// TerminalItem is the project menu's New terminal, the commands package's
// entry in the project_menu slot, which the application draws itself as it
// owns the terminals.
func (s *State) TerminalItem(c *ui.Context, m *ui.Menu, pr *preview.Project) {
	if !m.Item(i18n.T(c, "workspace_new_terminal")).Chosen() {
		return
	}
	switch {
	case s.Actions != nil && s.Actions.NewTerminal != nil:
		s.Actions.NewTerminal(pr)
	case s.Actions == nil && s.Sample.NewTerminal != nil:
		s.Sample.NewTerminal(pr)
	default:
		s.notice(c)
	}
}

// WorktreeMenu is the project menu's worktrees submenu, the worktrees
// package's entry in the project_menu slot, which the application draws
// itself: create one, open one's page, remove one.
func (s *State) WorktreeMenu(c *ui.Context, m *ui.Menu, pr *preview.Project) {
	t := func(key string) string { return i18n.T(c, key) }
	m.Submenu(t("worktree_manage"), func(m *ui.Menu) {
		if m.Item(t("worktree_create")).Chosen() {
			s.worktrees(c, pr, "create", "")
		}
		m.Separator()
		for _, w := range pr.Worktrees {
			if m.Item(w.Branch).Checked(w.Main).Chosen() {
				s.worktrees(c, pr, "select", w.Path)
			}
		}
		if len(pr.Worktrees) < 2 {
			return
		}
		m.Separator()
		if s.Actions != nil || !kit.Demo(c) {
			if m.Item(t("worktree_remove")).Chosen() {
				s.worktrees(c, pr, "remove", "")
			}
			return
		}
		// The live window picks the worktree in the Git panel; the demo
		// names them here.
		m.Submenu(t("worktree_remove"), func(m *ui.Menu) {
			for _, w := range pr.Worktrees {
				if !w.Main && m.Item(w.Branch).Chosen() {
					s.worktrees(c, pr, "remove", w.Path)
				}
			}
		})
	})
}

// worktrees runs a worktree operation of a project's menus: the live
// adapter's, or the demo's in memory. Creating adds a worktree and opens a
// new session in the project, as the live window does; selecting opens the
// project's page; removing asks first, naming the worktree. The preview
// tells its notice.
func (s *State) worktrees(c *ui.Context, pr *preview.Project, op, path string) {
	if s.Actions != nil && s.Actions.Worktrees != nil {
		s.Actions.Worktrees(pr, op, path)
		return
	}
	if s.Actions != nil || !kit.Demo(c) {
		s.notice(c)
		return
	}
	switch op {
	case "create":
		s.addWorktree(pr)
		s.newIn(pr)
	case "select":
		s.OpenOverview(c, pr)
	case "remove":
		i := slices.IndexFunc(pr.Worktrees, func(w preview.Worktree) bool { return !w.Main && (path == "" || w.Path == path) })
		if i < 0 {
			return
		}
		tree := pr.Worktrees[i]
		s.confirm = confirm{open: true, title: i18n.T(c, "worktree_remove"), message: tree.Path + "\n" + i18n.T(c, "worktree_remove_confirm"),
			action: i18n.T(c, "worktree_remove"), run: func() {
				pr.Worktrees = slices.DeleteFunc(pr.Worktrees, func(w preview.Worktree) bool { return w == tree })
			}}
	}
}

// addWorktree adds a worktree to a sample project, in memory: a branch named
// after the project's sessions made here, beside the project's folder.
func (s *State) addWorktree(pr *preview.Project) preview.Worktree {
	if len(pr.Worktrees) == 0 {
		branch := pr.Branch
		if branch == "" {
			branch = "main"
		}
		pr.Worktrees = []preview.Worktree{{Branch: branch, Path: pr.Path, Main: true}}
	}
	s.created++
	tree := preview.Worktree{Branch: fmt.Sprintf("worktree-%d", s.created), Path: fmt.Sprintf("%s-worktree-%d", pr.Path, s.created)}
	pr.Worktrees = append(pr.Worktrees, tree)
	return tree
}

// fork copies a sample session beside itself, in memory, and shows the copy:
// in the session's own worktree, or in a new one.
func (s *State) fork(pr *preview.Project, cv *preview.Conversation, worktree bool) {
	if pr != nil {
		s.Fork(cv, "", worktree)
	}
}

// Fork copies a sample session beside itself, in memory, and shows the copy:
// all of it, or its turns up to turn, where the copy is at rest; in the
// session's own worktree, or in a new one of its project.
func (s *State) Fork(cv *preview.Conversation, turn string, worktree bool) {
	pr := s.Data.ProjectOf(cv.ID)
	s.created++
	fork := *cv
	owner := "chat"
	if pr != nil {
		owner = pr.ID
	}
	fork.ID = fmt.Sprintf("%s/fork-%d", owner, s.created)
	fork.Turns = slices.Clone(cv.Turns)
	if at := slices.IndexFunc(fork.Turns, func(t conversation.Turn) bool { return t.ID == turn }); at >= 0 {
		fork.Turns = fork.Turns[:at+1]
		fork.Status, fork.Waiting, fork.Queue, fork.Animating = preview.Idle, composer.Interactions{}, nil, false
		if last := &fork.Turns[at]; last.Reply.Status.Active() {
			last.Reply.Status, last.Reply.Phase = conversation.Cancelled, conversation.PhaseNone
		}
	}
	fork.Pinned, fork.Updated, fork.Created = false, preview.Now, 10000+s.created
	if pr == nil {
		at := slices.Index(s.Data.Unassigned, cv)
		s.Data.Unassigned = slices.Insert(s.Data.Unassigned, max(at, 0), &fork)
		s.show(fork.ID)
		return
	}
	if worktree {
		tree := s.addWorktree(pr)
		fork.Worktree, fork.WorktreePath = tree.Branch, tree.Path
	}
	at := slices.Index(pr.Conversations, cv)
	pr.Conversations = slices.Insert(pr.Conversations, max(at, 0), &fork)
	*s.opened("project/" + pr.ID) = true
	s.show(fork.ID)
}

// AddWorktree adds a worktree to a sample project, in memory.
func (s *State) AddWorktree(pr *preview.Project) preview.Worktree { return s.addWorktree(pr) }

// archive asks before archiving, which in the preview only hides the row
// (workspace_memory_only).
func (s *State) archive(c *ui.Context, cv *preview.Conversation) {
	if s.Actions != nil && s.Actions.Archive != nil {
		s.confirm = confirm{open: true, title: i18n.T(c, "workspace_archive"), message: i18n.T(c, "session_archive_named", "name", Title(c, cv)), action: i18n.T(c, "workspace_archive"), run: func() { s.Actions.Archive(cv) }}
		return
	}
	// The demo asks as the application does.
	message, action := i18n.T(c, "workspace_memory_only"), i18n.T(c, "workspace_confirm")
	if kit.Demo(c) {
		message, action = i18n.T(c, "session_archive_named", "name", Title(c, cv)), i18n.T(c, "workspace_archive")
	}
	s.confirm = confirm{open: true, title: i18n.T(c, "workspace_archive"), message: message,
		action: action, run: func() {
			cv.Archived = true
			if s.Selected == cv.ID {
				if ids := s.Order(); len(ids) > 0 {
					s.Selected = ids[0]
				}
			}
		}}
}

// delete takes a conversation out of the preview's memory
// (RemoveSession); its split pane leaves with it.
func (s *State) delete(cv *preview.Conversation) {
	if s.Actions != nil && s.Actions.Delete != nil {
		s.Actions.Delete(cv)
		return
	}
	drop := func(list []*preview.Conversation) []*preview.Conversation {
		return slices.DeleteFunc(list, func(x *preview.Conversation) bool { return x == cv })
	}
	for _, p := range s.Data.Projects {
		p.Conversations = drop(p.Conversations)
	}
	s.Data.Unassigned = drop(s.Data.Unassigned)
	if s.Selected == cv.ID {
		s.Selected = ""
		if ids := s.Order(); len(ids) > 0 {
			s.Selected = ids[0]
		}
	}
}

// AskRemove confirms a project's removal, naming it and what follows. The
// preview keeps a project its sessions still use, and says so instead; the
// demo removes it as a live window does, its sessions leaving the list with
// it.
func (s *State) AskRemove(c *ui.Context, pr *preview.Project) {
	message := i18n.T(c, "project_remove_named", "name", pr.Path)
	if s.Actions == nil {
		if len(pr.Conversations) > 0 && !kit.Demo(c) {
			kit.Toast(c, kit.Failed, i18n.T(c, "project_remove_sessions"))
			return
		}
		// The demo asks as a live window does.
		if !kit.Demo(c) {
			message = i18n.T(c, "project_remove_sample", "name", pr.Path)
		}
	}
	s.confirm = confirm{open: true, title: i18n.T(c, "project_remove"), message: message, action: i18n.T(c, s.removeLabel(c)),
		run: func() { s.remove(pr) }}
}

// remove takes a project out of its Node's inventory through the live
// adapter, or out of the preview's memory.
func (s *State) remove(pr *preview.Project) {
	if s.Actions != nil && s.Actions.RemoveProject != nil {
		s.Actions.RemoveProject(pr)
		return
	}
	for i, p := range s.Data.Projects {
		if p == pr {
			s.Data.Projects = append(s.Data.Projects[:i:i], s.Data.Projects[i+1:]...)
		}
	}
	if s.Data.Find(s.Selected) == nil {
		if ids := s.Order(); len(ids) > 0 {
			s.Selected = ids[0]
		}
	}
}

// StartRename edits a conversation's title in place.
func (s *State) StartRename(cv *preview.Conversation) {
	s.renaming, s.draft = cv.ID, cv.Title
	s.renameTarget = cv
	s.renamePending = false
	s.renameGeneration++
}

// renameRow is the row of the conversation renamed: an input with its
// title, which Enter or leaving it keeps, and Escape gives up. An empty
// title stays in the input with a toast.
func (s *State) renameRow(c *ui.Context, cv *preview.Conversation) {
	p := theme.Of(c)
	commit := func() {
		if s.renaming != cv.ID || s.renamePending {
			return
		}
		title, ok := ValidTitle(s.draft)
		if !ok {
			kit.Toast(c, kit.Failed, i18n.T(c, "session_title_required"))
			return
		}
		if s.Actions != nil && s.Actions.Rename != nil {
			s.renamePending = true
			generation := s.renameGeneration
			s.Actions.Rename(s.renameTarget, title, func(ok bool) {
				if generation != s.renameGeneration {
					return
				}
				s.renamePending = false
				if ok && s.renaming == cv.ID {
					s.renaming = ""
				}
			})
			return
		}
		cv.Title, s.renaming = title, ""
	}
	ui.Row(c.Key(cv.ID)).Height(rowHeight).PaddingStart(indent - 6).PaddingEnd(4).Children(func() {
		in := ui.TextInputBase(c.Key("rename"), &s.draft).Grow(1).Height(24).PaddingX(6).Radius(p.Radius).
			Border(1, p.Ring).Background(p.Background).TextColor(p.Foreground).FontSize(14).
			Label(i18n.T(c, "workspace_rename")).AutoFocus().Disabled(s.renamePending)
		in.OnSubmit(commit)
		if in.Shortcut(0, ui.KeyEscape) {
			s.renaming = ""
		}
		if in.FocusLeft() && s.renaming == cv.ID && !s.renamePending {
			// Leaving the input keeps a valid title and gives up an empty one.
			if title, ok := ValidTitle(s.draft); ok {
				if s.Actions != nil && s.Actions.Rename != nil {
					commit()
					return
				}
				cv.Title = title
			}
			s.renaming = ""
		}
	})
}

// dialogs are the sidebar's project editor and its confirmation.
func (s *State) dialogs(c *ui.Context) {
	s.editorDialog(c)
	s.recordsDialog(c)
	if s.confirm.open {
		if kit.AlertDialog(c.Key("confirm"), &s.confirm.open, s.confirm.title, s.confirm.message, i18n.T(c, "settings_cancel"),
			s.confirm.action) == 1 {
			s.confirm.run()
		}
	}
}
