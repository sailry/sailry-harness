package sidebar

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// Dialogs builds the sidebar's editor and confirmations when View is absent.
func Dialogs(c *ui.Context, s *State) { s.dialogs(c) }

func (s *State) dialogs(c *ui.Context) {
	s.editorDialog(c)
	if !s.confirm.open {
		return
	}
	answer := kit.AlertDialog(c.Key("confirm"), &s.confirm.open, s.confirm.title, s.confirm.message, i18n.T(c, "settings_cancel"), s.confirm.action)
	if answer < 0 {
		return
	}
	run := s.confirm.run
	s.confirm = confirm{}
	if answer == 1 {
		run()
	}
}

func (s *State) confirmDelete(c *ui.Context, targets ...*preview.Conversation) {
	if len(targets) == 0 || s.confirm.open {
		return
	}
	var message string
	if len(targets) == 1 {
		cv := targets[0]
		key := "session_delete_named"
		if s.Data.ProjectHolding(cv) == nil {
			key = "session_delete_private_named"
		}
		message = i18n.T(c, key, "name", Title(c, cv))
	} else {
		message = i18n.T(c, "session_delete_many_confirm", "count", len(targets))
	}
	// The Node stops running sessions before it removes them.
	running := 0
	for _, cv := range targets {
		if cv.Status == viewmodel.Running || cv.Status == viewmodel.Waiting {
			running++
		}
	}
	switch {
	case running == 0:
	case len(targets) == 1:
		message += " " + i18n.T(c, "session_delete_running_one")
	default:
		message += " " + i18n.T(c, "session_delete_running_many", "count", running)
	}
	var run func()
	if s.Actions == nil {
		rows := slices.Clone(targets)
		run = func() { s.deletePreview(rows) }
	} else if remove := s.Actions.Delete; remove != nil {
		rows := make([]*preview.Conversation, 0, len(targets))
		for _, cv := range targets {
			rows = append(rows, &preview.Conversation{ID: cv.ID, Node: cv.Node, Revision: cv.Revision})
		}
		run = func() { remove(rows) }
	} else {
		s.notice(c)
		return
	}
	s.confirm = confirm{open: true, title: i18n.T(c, "session_delete"), message: message, action: i18n.T(c, "settings_delete"), run: run}
}

func (s *State) deletePreview(targets []*preview.Conversation) {
	removed := make(map[*preview.Conversation]bool, len(targets))
	for _, cv := range targets {
		removed[cv] = true
	}
	selected := removed[s.Current()]
	drop := func(list []*preview.Conversation) []*preview.Conversation {
		return slices.DeleteFunc(list, func(cv *preview.Conversation) bool { return removed[cv] })
	}
	for _, pr := range s.Data.Projects {
		pr.Conversations = drop(pr.Conversations)
	}
	s.Data.Unassigned = drop(s.Data.Unassigned)
	if selected {
		s.SelectOn("", "")
		if ids := s.Order(); len(ids) > 0 {
			s.SelectOn("", ids[0])
		}
	}
}
