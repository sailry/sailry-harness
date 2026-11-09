// Package sidebar is the conversation navigation beside the rail: pinned
// conversations, chats without a project, and projects with their
// conversations as a tree (sailry-public apps/desktop/src/sidebar.rs).
package sidebar

import (
	"fmt"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/theme"
)

// Limit is how many conversations a section shows before "Show more"
// (sidebar/limits.rs INITIAL).
const Limit = 5

// Row metrics of sidebar.rs and sidebar/tree.rs.
const (
	rowHeight = 28
	indent    = 30 // conversation rows under a project
	guideX    = 16 // the tree guide, from the row's left edge
)

// State is the sidebar's part of the app state.
type State struct {
	Data     *preview.Data
	Selected string // conversation ID
	open     map[string]*bool
	more     map[string]bool
	created  int
}

func New(d *preview.Data, selected string) *State {
	return &State{Data: d, Selected: selected, open: map[string]*bool{}, more: map[string]bool{}}
}

// opened returns the expansion flag of a section or project, open at first.
func (s *State) opened(key string) *bool {
	if b, ok := s.open[key]; ok {
		return b
	}
	b := true
	s.open[key] = &b
	return &b
}

// View builds the navigation column's content.
func View(c *ui.Context, s *State) {
	ui.Column(c).Padding(16, 12, 8, 12).Children(func() {
		kit.Primary(c.Key("new"), i18n.T(c, "conversation"), false).FillWidth().Children(func() {
			ui.Icon(c, icons.Plus).FontSize(16)
			ui.Text(c, i18n.T(c, "conversation")).FontSize(14).SingleLine()
		}).OnClick(s.newChat)
	})
	ui.Scroll(c.Key("sessions")).Grow(1).Padding(8, 12).Gap(4).Children(func() {
		var pinned []*preview.Conversation
		for _, pr := range s.Data.Projects {
			for _, cv := range pr.Conversations {
				if cv.Pinned {
					pinned = append(pinned, cv)
				}
			}
		}
		if len(pinned) > 0 {
			section(c, s, "pinned", i18n.T(c, "sidebar_pinned"), func() {
				s.limited(c, "pinned", pinned, func(cv *preview.Conversation, last bool) {
					s.conversation(c, cv, true, false)
				})
			})
		}
		section(c, s, "unassigned", i18n.T(c, "sessions_unassigned"), func() {
			s.limited(c, "unassigned", s.Data.Unassigned, func(cv *preview.Conversation, last bool) {
				s.conversation(c, cv, false, false)
			})
		})
		section(c, s, "projects", i18n.T(c, "projects"), func() {
			ui.Column(c).Gap(2).Children(func() {
				for _, pr := range s.Data.Projects {
					s.project(c, pr)
				}
			})
		})
	})
}

// section is a collapsible group under a muted title whose chevron shows
// while the pointer is over it (sidebar.rs section headers).
func section(c *ui.Context, s *State, key, title string, body func()) {
	p := theme.Of(c)
	ui.Column(c.Key(key)).Children(func() {
		parts := ui.CollapsibleBase(c, s.opened(key))
		hovered := parts.Trigger.Hovered()
		parts.Trigger.Height(32).PaddingX(8).Gap(4).Radius(6).Cursor(ui.CursorPointer).
			TextColor(p.MutedForeground.Alpha(0.7)).Children(func() {
			ui.Text(c, title).FontSize(14).SingleLine()
			chevron := ui.Icon(c, icons.ChevronDown).FontSize(12).Rotate(-90 * (1 - parts.Progress()))
			if !hovered {
				chevron.Opacity(0)
			}
		})
		parts.Panel(body)
	})
}

// limited shows the first Limit conversations of a list and a toggle for
// the rest.
func (s *State) limited(c *ui.Context, key string, list []*preview.Conversation, row func(cv *preview.Conversation, last bool)) {
	n := len(list)
	if !s.more[key] && n > Limit {
		n = Limit
	}
	for i := range n {
		row(list[i], i == n-1)
	}
	if len(list) > Limit {
		p := theme.Of(c)
		label := i18n.T(c, "sidebar_sessions_more")
		if s.more[key] {
			label = i18n.T(c, "sidebar_sessions_less")
		}
		kit.Ghost(c.Key(key+"/more"), label, false).FillWidth().Height(28).PaddingStart(indent).Justify(ui.Start).
			TextColor(p.MutedForeground.Alpha(0.7)).Children(func() {
			ui.Text(c, label).FontSize(14).SingleLine()
		}).OnClick(func() { s.more[key] = !s.more[key] })
	}
}

func (s *State) project(c *ui.Context, pr *preview.Project) {
	p := theme.Of(c)
	open := s.opened("project/" + pr.ID)
	ui.Column(c.Key(pr.ID)).Gap(2).Children(func() {
		b := ui.ButtonBase(c).Label(pr.Name).Height(rowHeight).PaddingX(8).Gap(8).Radius(6).TextColor(p.Foreground)
		hovered := b.Hovered()
		b.Children(func() {
			ui.Icon(c, icons.Folder).FontSize(14).TextColor(p.MutedForeground)
			ui.Text(c, pr.Name).FontSize(14).SingleLine().Grow(1)
			chevron := ui.Icon(c, icons.ChevronDown).FontSize(14).TextColor(p.MutedForeground)
			if !*open {
				chevron.Rotate(-90)
			}
			if !hovered {
				chevron.Opacity(0)
			}
		}).OnClick(func() { *open = !*open })
		if !*open {
			return
		}
		var list []*preview.Conversation
		for _, cv := range pr.Conversations {
			if !cv.Pinned {
				list = append(list, cv)
			}
		}
		if len(list) == 0 {
			ui.Text(c, i18n.T(c, "project_sessions_empty")).FontSize(14).TextColor(p.MutedForeground).Padding(4, 8)
			return
		}
		s.limited(c, pr.ID, list, func(cv *preview.Conversation, last bool) {
			s.conversation(c, cv, false, true, last)
		})
	})
}

// conversation is a session row: title, age, and on hover a pin button. In
// a project it hangs from the tree guide, its status on the elbow.
func (s *State) conversation(c *ui.Context, cv *preview.Conversation, pinned, tree bool, last ...bool) {
	p := theme.Of(c)
	selected := cv.ID == s.Selected
	title := Title(c, cv)
	b := ui.ButtonBase(c.Key(cv.ID)).Label(title).Height(rowHeight).PaddingStart(indent).PaddingEnd(8).Gap(8).Radius(6)
	hovered := b.Hovered()
	b.Background(p.SidebarItem(selected, hovered))
	if selected {
		b.AccessSelected(true)
	}
	text := p.MutedForeground
	if selected || hovered || cv.Status == preview.Running {
		text = p.Foreground
	}
	b.TextColor(text).Children(func() {
		if tree {
			guide(c, len(last) > 0 && last[0])
		}
		status(c, cv.Status)
		ui.Text(c, title).FontSize(14).SingleLine().Grow(1).Shrink(1)
		if hovered {
			icon, label := icons.Pin, i18n.T(c, "workspace_pin")
			if cv.Pinned {
				label = i18n.T(c, "workspace_unpin")
			}
			kit.IconButton(c.Key("pin"), icon, label, 20, false).TextColor(p.MutedForeground).
				OnClick(func() { cv.Pinned = !cv.Pinned })
			return
		}
		if pinned {
			if pr := s.Data.ProjectOf(cv.ID); pr != nil {
				ui.Text(c, pr.Name).FontSize(12).SingleLine().MaxWidth(80).TextColor(p.MutedForeground)
			}
		}
		ui.Text(c, Age(c, cv.Updated)).FontSize(12).SingleLine().TextColor(p.MutedForeground.Alpha(0.6))
	}).OnClick(func() { s.Selected = cv.ID })
}

// guide draws the dashed tree line and its arm to the row (sidebar/tree.rs):
// the line runs past the row by 4 DIPs and stops at the arm on the last row.
func guide(c *ui.Context, last bool) {
	color := theme.Of(c).Foreground.Alpha(0.2)
	h := float32(rowHeight + 8)
	if last {
		h = rowHeight/2 + 4
	}
	ui.Box(c).Absolute().Left(guideX).Top(-4).Width(1).Height(h).
		BorderWidth(0, 0, 0, 1).BorderStyle(ui.BorderDashed).BorderColor(color).PassThrough()
	ui.Box(c).Absolute().Left(guideX).Top(rowHeight/2).Width(8).Height(1).
		BorderWidth(1, 0, 0, 0).BorderStyle(ui.BorderDashed).BorderColor(color).PassThrough()
}

// status puts the conversation's state in the 12x12 slot on the elbow.
func status(c *ui.Context, st preview.Status) {
	p := theme.Of(c)
	slot := ui.Box(c).Absolute().Left(guideX-6).Top(rowHeight/2-6).Size(12, 12).Center().PassThrough()
	switch st {
	case preview.Running:
		slot.Label(i18n.T(c, "activity_running")).Children(func() { kit.Loader(c, 12) })
	case preview.Waiting:
		slot.Label(i18n.T(c, "activity_waiting")).Children(func() {
			ui.Icon(c, icons.Info).FontSize(12).TextColor(p.Warning)
		})
	case preview.Unread:
		slot.Label(i18n.T(c, "notification_unread")).Children(func() {
			ui.Box(c).Size(8, 8).Radius(4).Background(p.Primary)
		})
	}
}

// Age is the short age of sidebar rows: 45s, 12m, 3h, 2d.
func Age(c *ui.Context, t time.Time) string {
	d := preview.Now.Sub(t)
	switch {
	case d < time.Minute:
		return i18n.T(c, "sidebar_age_seconds", "count", int(d.Seconds()))
	case d < time.Hour:
		return i18n.T(c, "sidebar_age_minutes", "count", int(d.Minutes()))
	case d < 24*time.Hour:
		return i18n.T(c, "sidebar_age_hours", "count", int(d.Hours()))
	}
	return i18n.T(c, "sidebar_age_days", "count", int(d.Hours()/24))
}

// newChat starts an empty chat without a project, in memory only.
func (s *State) newChat() {
	s.created++
	cv := &preview.Conversation{ID: fmt.Sprintf("chat/new-%d", s.created), Updated: preview.Now}
	s.Data.Unassigned = append([]*preview.Conversation{cv}, s.Data.Unassigned...)
	s.Selected = cv.ID
}

// Title is a conversation's title, or the "New conversation" copy while it
// has none.
func Title(c *ui.Context, cv *preview.Conversation) string {
	if cv.Title == "" {
		return i18n.T(c, "conversation")
	}
	return cv.Title
}
