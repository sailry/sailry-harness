package sidebar

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// notice tells that an action would leave the preview: it would write
// files, run git, a terminal or a model.
func notice(c *ui.Context) { kit.Message(c, i18n.T(c, "preview_notice")) }

func (s *State) notice(c *ui.Context) {
	if s.Actions != nil {
		kit.Notify(c, "", i18n.T(c, "unavailable"))
	} else {
		notice(c)
	}
}

// sessionMenu is a session row's right-click menu, the system's, with the
// reference's items in its order. Actions on the preview's rows change them
// in memory; the rest tell the preview notice.
func (s *State) sessionMenu(c *ui.Context, m *ui.Menu, cv *preview.Conversation) {
	t := func(key string) string { return i18n.T(c, key) }
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
		s.confirm = confirm{open: true, title: t("session_delete"), message: i18n.T(c, "session_delete_named", "name", Title(c, cv)),
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
	m.Separator()
	busy := cv.Status == preview.Running || cv.Status == preview.Waiting
	m.Submenu(t("session_fork"), func(m *ui.Menu) {
		if m.Item(t("session_fork_here")).Disabled(busy || pr == nil).Chosen() {
			if s.Actions != nil && s.Actions.ForkHere != nil {
				s.Actions.ForkHere(cv)
			} else {
				s.notice(c)
			}
		}
		if m.Item(t("session_fork_worktree")).Disabled(busy || pr == nil).Chosen() {
			if s.Actions != nil && s.Actions.ForkWorktree != nil {
				s.Actions.ForkWorktree(cv)
			} else {
				s.notice(c)
			}
		}
	})
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

// projectMenu is a project row's right-click menu, the system's.
func (s *State) projectMenu(c *ui.Context, m *ui.Menu, pr *preview.Project) {
	t := func(key string) string { return i18n.T(c, key) }
	if m.Item(t("workspace_project")).Chosen() {
		if s.Actions != nil && s.Actions.Overview != nil {
			s.Actions.Overview(pr)
		} else {
			s.notice(c)
		}
	}
	m.Separator()
	if m.Item(t("workspace_new_session")).Chosen() {
		s.newIn(pr)
	}
	for _, key := range []string{"workspace_new_terminal", "workspace_records"} {
		if m.Item(t(key)).Chosen() {
			if key == "workspace_records" && s.Actions != nil && s.Actions.Records != nil {
				s.Actions.Records(pr)
			} else if key == "workspace_new_terminal" && s.Actions != nil && s.Actions.NewTerminal != nil {
				s.Actions.NewTerminal(pr)
			} else {
				s.notice(c)
			}
		}
	}
	if m.Item(t("project_edit")).Chosen() {
		s.OpenEditor(pr)
	}
	if s.Actions == nil && m.Item(t("project_trust")).Chosen() {
		s.trust(c, pr)
	}
	if m.Item(t("workspace_copy_path")).Chosen() {
		c.WriteClipboard(pr.Path)
	}
	m.Submenu(t("worktree_manage"), func(m *ui.Menu) {
		if m.Item(t("worktree_create")).Chosen() {
			if s.Actions != nil && s.Actions.Worktrees != nil {
				s.Actions.Worktrees(pr, "create", "")
			} else {
				s.notice(c)
			}
		}
		m.Separator()
		for _, w := range pr.Worktrees {
			if m.Item(w.Branch).Checked(w.Main).Chosen() {
				if s.Actions != nil && s.Actions.Worktrees != nil {
					s.Actions.Worktrees(pr, "select", w.Path)
				} else {
					s.notice(c)
				}
			}
		}
		if len(pr.Worktrees) > 1 {
			m.Separator()
			if m.Item(t("worktree_remove")).Chosen() {
				if s.Actions != nil && s.Actions.Worktrees != nil {
					s.Actions.Worktrees(pr, "remove", "")
				} else {
					s.notice(c)
				}
			}
		}
	})
	m.Separator()
	if m.Item(t("project_remove")).Chosen() {
		s.AskRemove(c, pr)
	}
	if m.Item(t("cli_launch")).Chosen() {
		if s.Actions != nil && s.Actions.CLI != nil {
			s.Actions.CLI(pr)
		} else {
			s.notice(c)
		}
	}
}

// archive asks before archiving, which in the preview only hides the row
// (workspace_memory_only).
func (s *State) archive(c *ui.Context, cv *preview.Conversation) {
	if s.Actions != nil && s.Actions.Archive != nil {
		s.confirm = confirm{open: true, title: i18n.T(c, "workspace_archive"), message: i18n.T(c, "session_archive_named", "name", Title(c, cv)), action: i18n.T(c, "workspace_archive"), run: func() { s.Actions.Archive(cv) }}
		return
	}
	s.confirm = confirm{open: true, title: i18n.T(c, "workspace_archive"), message: i18n.T(c, "workspace_memory_only"),
		action: i18n.T(c, "workspace_confirm"), run: func() {
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
// preview keeps a project its sessions still use, and says so instead.
func (s *State) AskRemove(c *ui.Context, pr *preview.Project) {
	message := i18n.T(c, "project_remove_named", "name", pr.Path)
	if s.Actions == nil {
		if len(pr.Conversations) > 0 {
			kit.Toast(c, kit.Failed, i18n.T(c, "project_remove_sessions"))
			return
		}
		message = i18n.T(c, "project_remove_sample", "name", pr.Path)
	}
	s.confirm = confirm{open: true, title: i18n.T(c, "project_remove"), message: message, action: i18n.T(c, s.removeLabel()),
		run: func() { s.remove(pr) }}
}

// trust asks before granting or revoking a sample project's trust, which
// the preview only records.
func (s *State) trust(c *ui.Context, pr *preview.Project) {
	title, detail, action := "project_grant_title", "project_grant_description", "project_grant"
	if pr.Trusted {
		title, detail, action = "project_revoke_title", "project_revoke_description", "project_revoke"
	}
	trusted, path := pr.Trusted, pr.Path
	s.confirm = confirm{open: true, title: i18n.T(c, title),
		message: pr.Name + "\n" + pr.Path + "\n" + i18n.T(c, detail) + "\n" + i18n.T(c, "project_trust_preview"),
		action:  i18n.T(c, action), run: func() {
			if pr.Path == path && pr.Trusted == trusted {
				pr.Trusted = !trusted
			}
		}}
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
