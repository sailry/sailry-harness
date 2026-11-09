// Package sidebar is the conversation navigation beside the rail: pinned
// conversations, chats without a project, projects with their
// conversations as a tree, and recent conversations
// (sailry-public apps/desktop/src/sidebar.rs, sidebar/*.rs,
// workspace/menus.rs). See docs/sidebar.md.
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

// Limit is how many conversations a group shows before "Show more".
const Limit = Initial

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

	renaming string // the conversation renamed in place
	draft    string // its title as typed
	drawn    bool   // its input is built, in its first row, this frame

	editor  Editor
	confirm confirm
}

// confirm is the one confirmation the sidebar shows at a time: archive in
// the preview, or removing a project.
type confirm struct {
	open                   bool
	title, message, action string
	run                    func()
}

func New(d *preview.Data, selected string) *State {
	s := &State{Data: d, Selected: selected, open: map[string]*bool{}, more: map[string]bool{}}
	// The recent group starts closed (sidebar.rs recent_open).
	closed := false
	s.open["recent"] = &closed
	return s
}

// opened returns the expansion flag of a group or project, open at first.
func (s *State) opened(key string) *bool {
	if b, ok := s.open[key]; ok {
		return b
	}
	b := true
	s.open[key] = &b
	return &b
}

// Collapsed returns the projects whose conversations are hidden, which the
// shell keeps across launches.
func (s *State) Collapsed() []string {
	var out []string
	for _, p := range s.Data.Projects {
		if b, ok := s.open["project/"+p.ID]; ok && !*b {
			out = append(out, p.ID)
		}
	}
	return out
}

// SetCollapsed hides the conversations of the projects ids.
func (s *State) SetCollapsed(ids []string) {
	for _, id := range ids {
		closed := false
		s.open["project/"+id] = &closed
	}
}

// Reveal opens the projects group and a project's rows.
func (s *State) Reveal(id string) {
	*s.opened("projects") = true
	*s.opened("project/" + id) = true
}

// pinned returns the pinned group's conversations in their order.
func (s *State) pinned() []*preview.Conversation {
	byID := map[string]*preview.Conversation{}
	var pins []Pin
	s.each(func(cv *preview.Conversation) {
		if cv.Pinned && !cv.Archived {
			byID[cv.ID] = cv
			pins = append(pins, Pin{ID: cv.ID, Order: cv.PinOrder, LastRun: cv.Updated, Created: cv.Created})
		}
	})
	var out []*preview.Conversation
	for _, p := range OrderPins(pins) {
		out = append(out, byID[p.ID])
	}
	return out
}

// recent returns the conversations not archived, newest first.
func (s *State) recent() []*preview.Conversation {
	byID := map[string]*preview.Conversation{}
	var list []Recent
	s.each(func(cv *preview.Conversation) {
		byID[cv.ID] = cv
		list = append(list, Recent{ID: cv.ID, Created: cv.Created, Archived: cv.Archived})
	})
	var out []*preview.Conversation
	for _, r := range RecentOrder(list) {
		out = append(out, byID[r.ID])
	}
	return out
}

// each runs fn on every conversation: those without a project, then each
// project's.
func (s *State) each(fn func(cv *preview.Conversation)) {
	for _, cv := range s.Data.Unassigned {
		fn(cv)
	}
	for _, p := range s.Data.Projects {
		for _, cv := range p.Conversations {
			fn(cv)
		}
	}
}

func listed(list []*preview.Conversation) []*preview.Conversation {
	var out []*preview.Conversation
	for _, cv := range list {
		if !cv.Archived {
			out = append(out, cv)
		}
	}
	return out
}

// Order is the order of next and previous conversation: pinned, chats,
// each project's, then recent, without repeats.
func (s *State) Order() []string {
	seen := map[string]bool{}
	var ids []string
	add := func(list []*preview.Conversation) {
		for _, cv := range list {
			if !seen[cv.ID] {
				seen[cv.ID] = true
				ids = append(ids, cv.ID)
			}
		}
	}
	add(s.pinned())
	add(listed(s.Data.Unassigned))
	for _, p := range s.Data.Projects {
		add(listed(p.Conversations))
	}
	add(s.recent())
	return ids
}

// View builds the navigation column's content.
func View(c *ui.Context, s *State) {
	s.drawn = false
	ui.Column(c).Padding(16, 12, 8, 12).Children(func() {
		b := kit.Primary(c.Key("new"), i18n.T(c, "conversation"), false).FillWidth()
		kit.NewSessionArt(c, b).Children(func() {
			ui.Icon(c, icons.Plus).FontSize(16)
			ui.Text(c, i18n.T(c, "conversation")).FontSize(14).SingleLine()
		}).OnClick(s.NewChat)
	})
	ui.Scroll(c.Key("sessions")).Grow(1).Padding(8, 12).Gap(4).Children(func() {
		if pinned := s.pinned(); len(pinned) > 0 {
			section(c, s, "pinned", i18n.T(c, "sidebar_pinned"), nil, func() {
				s.limited(c, "pinned", pinned, func(cv *preview.Conversation, last bool) {
					s.conversation(c, cv, rowPinned, last)
				})
			})
		}
		section(c, s, "unassigned", i18n.T(c, "sessions_unassigned"), &add{"workspace_new_session", s.NewChat}, func() {
			list := listed(s.Data.Unassigned)
			if len(list) == 0 {
				empty(c, "project_sessions_empty")
				return
			}
			s.limited(c, "unassigned", list, func(cv *preview.Conversation, last bool) {
				s.conversation(c, cv, rowPlain, last)
			})
		})
		section(c, s, "projects", i18n.T(c, "projects"), &add{"project_add", func() { s.OpenEditor(nil) }}, func() {
			if len(s.Data.Projects) == 0 {
				empty(c, "connection_no_projects")
				return
			}
			ui.Column(c).Gap(2).Children(func() {
				for _, pr := range s.Data.Projects {
					s.project(c, pr)
				}
			})
		})
		section(c, s, "recent", i18n.T(c, "recent_sessions"), nil, func() {
			for _, cv := range s.recent() {
				s.conversation(c.Key("recent"), cv, rowRecent, false)
			}
		})
	})
	s.dialogs(c)
}

func empty(c *ui.Context, key string) {
	ui.Text(c, i18n.T(c, key)).FontSize(14).Padding(4, 8, 4, indent).TextColor(theme.Of(c).MutedForeground.Alpha(0.7))
}

// add is a group header's "+" button.
type add struct {
	label string // i18n key
	run   func()
}

// groupIcons are the theme's icon.group.* slots of the groups; pinned
// conversations take the recent group's.
var groupIcons = map[string]string{"pinned": "group.recent", "unassigned": "group.unassigned", "projects": "group.projects", "recent": "group.recent"}

// section is a collapsible group under a muted title whose chevron, and
// "+" when it has one, show while the pointer is over it (sidebar.rs). A
// theme with an icon for the group shows it before the title. The group
// grows open and closed (MyGo's collapsible motion).
func section(c *ui.Context, s *State, key, title string, plus *add, body func()) {
	p := theme.Of(c)
	ui.Column(c.Key(key)).Children(func() {
		muted := p.MutedForeground.Alpha(0.7)
		if key == "recent" {
			muted = p.SidebarForeground.Alpha(0.5)
		}
		var parts ui.CollapsibleParts
		ui.Row(c).Height(32).AlignItems(ui.Center).Children(func() {
			parts = ui.CollapsibleBase(c, s.opened(key))
			hovered := parts.Trigger.Hovered()
			parts.Trigger.Grow(1).Shrink(1).MinWidth(0).Height(32).PaddingX(8).Gap(4).Radius(p.Radius).Cursor(ui.CursorPointer).
				TextColor(muted).Children(func() {
				if icon := groupIcons[key]; kit.HasArtIcon(c, icon) {
					kit.ArtIcon(c, icon, nil, 16, ui.Color{}).Margin(0, 4, 0, 0)
				}
				ui.Text(c, title).FontSize(14).SingleLine()
				chevron := ui.Icon(c, icons.ChevronDown).FontSize(12).Rotate(-90 * (1 - parts.Progress()))
				if !hovered {
					chevron.Opacity(0)
				}
			})
			if plus != nil {
				b := kit.IconButton(c.Key("add"), icons.Plus, i18n.T(c, plus.label), 20, false).TextColor(muted).
					Margin(0, 4, 0, 0).OnClick(plus.run)
				if !hovered && !b.Hovered() && !b.FocusVisible() {
					b.Opacity(0)
				}
			}
		})
		parts.Panel(body)
	})
}

// limited shows the first Limit conversations of a list and a toggle for
// the rest (sidebar/limits.rs).
func (s *State) limited(c *ui.Context, key string, list []*preview.Conversation, row func(cv *preview.Conversation, last bool)) {
	n, more := Window(len(list), s.more[key])
	for i := range n {
		row(list[i], i == n-1)
	}
	if more {
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

// project is a project row and, while it is open, its conversations
// hanging from the tree guide. They grow in and out as it opens and
// closes. Its right-click menu is the system's (workspace/menus.rs).
func (s *State) project(c *ui.Context, pr *preview.Project) {
	p := theme.Of(c)
	ui.Column(c.Key(pr.ID)).Gap(2).Children(func() {
		parts := ui.CollapsibleBase(c, s.opened("project/"+pr.ID))
		hovered := parts.Trigger.Hovered()
		parts.Trigger.Label(pr.Name).Height(rowHeight).PaddingX(8).Gap(8).Radius(p.Radius).TextColor(p.SidebarForeground).
			Cursor(ui.CursorPointer).ContextMenu(func(m *ui.Menu) { s.projectMenu(c, m, pr) }).Children(func() {
			ui.Icon(c, icons.Folder).FontSize(14).TextColor(p.MutedForeground)
			ui.Text(c, pr.Name).FontSize(14).SingleLine().Grow(1).Shrink(1)
			label := i18n.T(c, "workspace_collapse")
			if !*s.opened("project/" + pr.ID) {
				label = i18n.T(c, "workspace_expand")
			}
			chevron := ui.Icon(c, icons.ChevronDown).FontSize(14).TextColor(p.MutedForeground).Label(label).
				Rotate(-90 * (1 - parts.Progress()))
			if !hovered {
				chevron.Opacity(0)
			}
		})
		parts.Panel(func() {
			list := listed(pr.Conversations)
			if len(list) == 0 {
				empty(c, "project_sessions_empty")
				return
			}
			ui.Column(c).Gap(2).Children(func() {
				s.limited(c, pr.ID, list, func(cv *preview.Conversation, last bool) {
					s.conversation(c, cv, rowTree, last)
				})
			})
		})
	})
}

// rowKind is where a conversation row is: its look and what it shows.
type rowKind int

const (
	rowPlain  rowKind = iota // chats without a project
	rowTree                  // under a project, on the tree guide
	rowPinned                // pinned: the project's name, and drags to reorder
	rowRecent                // recent: no status
)

// pinDrag is the value a pinned row drags.
type pinDrag struct{ id string }

// conversation is a session row: its status, title and age, and on hover
// a pin and an archive button. A double click renames it in place. Its
// right-click menu is the system's; the pointer resting on it shows its
// details in a card.
func (s *State) conversation(c *ui.Context, cv *preview.Conversation, kind rowKind, last bool) {
	p := theme.Of(c)
	if s.renaming == cv.ID && !s.drawn {
		s.drawn = true
		s.renameRow(c, cv)
		return
	}
	selected := cv.ID == s.Selected
	title := Title(c, cv)
	b := ui.ButtonBase(c.Key(cv.ID)).Label(title).Height(rowHeight).PaddingStart(indent).PaddingEnd(8).Gap(8).Radius(p.Radius)
	hovered := b.Hovered()
	b.Background(p.SidebarItem(selected, hovered))
	if selected {
		b.AccessSelected(true)
	}
	text := p.MutedForeground
	switch {
	case selected:
		text = p.SidebarAccentForeground
	case hovered || cv.Status == preview.Running && kind != rowRecent:
		text = p.SidebarForeground
	}
	if kind == rowPinned {
		s.reorder(c, b, cv)
	}
	b.ContextMenu(func(m *ui.Menu) { s.sessionMenu(c, m, cv) })
	kit.HoverCard(c, b, 320, func() { s.details(c, cv) })
	b.TextColor(text).Children(func() {
		if kind == rowTree {
			guide(c, last)
		}
		if kind != rowRecent {
			status(c, cv.Status)
		}
		ui.Text(c, title).FontSize(14).SingleLine().Grow(1).Shrink(1)
		if cv.Worktree != "" {
			ui.Icon(c, icons.Network).FontSize(14).TextColor(p.MutedForeground).Label(cv.Worktree)
		}
		if hovered {
			icon, label := icons.Pin, i18n.T(c, "workspace_pin")
			if cv.Pinned {
				icon, label = icons.PinOff, i18n.T(c, "workspace_unpin")
			}
			kit.IconButton(c.Key("pin"), icon, label, 20, false).TextColor(p.MutedForeground).
				OnClick(func() { s.TogglePin(cv) })
			kit.IconButton(c.Key("archive"), icons.Inbox, i18n.T(c, "workspace_archive"), 20, false).TextColor(p.MutedForeground).
				OnClick(func() { s.archive(c, cv) })
			return
		}
		if kind == rowPinned {
			if pr := s.Data.ProjectOf(cv.ID); pr != nil {
				ui.Text(c, pr.Name).FontSize(12).SingleLine().MaxWidth(80).TextColor(p.MutedForeground)
			}
		}
		if cv.Status != preview.Running {
			ui.Text(c, Age(c, cv.Updated)).FontSize(12).SingleLine().TextColor(p.MutedForeground.Alpha(0.6))
		}
	}).OnClick(func() { s.Selected = cv.ID })
	if b.DoubleClicked() {
		s.StartRename(cv)
	}
}

// reorder makes a pinned row drag to a new place among the pinned, a 2 DIP
// line showing where it would go (sidebar/order.rs's indicator).
func (s *State) reorder(c *ui.Context, b ui.Element, cv *preview.Conversation) {
	b.Drag(pinDrag{cv.ID})
	if b.Dragging() {
		b.Opacity(0.5)
	}
	if v, over := ui.DragOver[pinDrag](b); over && v.id != cv.ID {
		_, y, _ := b.PointerPosition()
		after := y >= b.Bounds().H/2
		line := ui.Box(c.Key("drop")).Absolute().Left(0).Right(0).Height(2).Radius(1).Background(theme.Of(c).Primary).PassThrough()
		if after {
			line.Bottom(-1)
		} else {
			line.Top(-1)
		}
	}
	if v, ok := ui.Drop[pinDrag](b); ok {
		_, y, _ := b.PointerPosition()
		s.MovePin(v.id, cv.ID, y >= b.Bounds().H/2)
	}
}

// MovePin moves a pinned conversation before or after another, giving the
// pins their new order.
func (s *State) MovePin(source, target string, after bool) {
	var ids []string
	for _, cv := range s.pinned() {
		ids = append(ids, cv.ID)
	}
	moved, ok := MovePin(ids, source, target, after)
	if !ok {
		return
	}
	for i, id := range moved {
		if cv := s.Data.Find(id); cv != nil {
			n := i
			cv.PinOrder = &n
		}
	}
}

// TogglePin pins a conversation at the end of the pinned, or unpins it.
func (s *State) TogglePin(cv *preview.Conversation) {
	if cv.Pinned {
		cv.Pinned, cv.PinOrder = false, nil
		return
	}
	var orders []*int
	for _, p := range s.pinned() {
		orders = append(orders, p.PinOrder)
	}
	n := NextPinOrder(orders)
	cv.Pinned, cv.PinOrder = true, &n
}

// details is the hover card of a session row (sidebar/details.rs): its
// title, then its project, branch and worktree path.
func (s *State) details(c *ui.Context, cv *preview.Conversation) {
	p := theme.Of(c)
	ui.Text(c, Title(c, cv)).FontSize(14).FontWeight(500).MaxLines(3)
	pr := s.Data.ProjectOf(cv.ID)
	if pr == nil {
		return
	}
	line := func(icon *ui.SVG, text string) {
		ui.Row(c).Gap(6).Children(func() {
			ui.Icon(c, icon).FontSize(12).TextColor(p.MutedForeground)
			ui.Text(c, text).FontSize(12).SingleLine().Shrink(1).TextColor(p.MutedForeground)
		})
	}
	ui.Column(c).Gap(4).Children(func() {
		line(icons.Folder, pr.Name)
		branch, path := pr.Branch, pr.Path
		for _, w := range pr.Worktrees {
			if cv.Worktree != "" && w.Branch == cv.Worktree {
				branch, path = w.Branch, w.Path
			}
		}
		line(icons.GitBranch, branch)
		ui.Text(c, path).FontSize(12).Font("monospace").TextColor(p.MutedForeground)
	})
}

// guide draws the dashed tree line and its arm to the row (sidebar/tree.rs):
// the line runs past the row by 4 DIPs and stops at the arm on the last row.
func guide(c *ui.Context, last bool) {
	color := theme.Of(c).SidebarForeground.Alpha(0.2)
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
	d := max(preview.Now.Sub(t), 0)
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

// NewChat starts an empty chat without a project, in memory only.
func (s *State) NewChat() {
	s.created++
	cv := &preview.Conversation{ID: fmt.Sprintf("chat/new-%d", s.created), Updated: preview.Now, Created: 10000 + s.created}
	s.Data.Unassigned = append([]*preview.Conversation{cv}, s.Data.Unassigned...)
	s.Selected = cv.ID
}

// newIn starts an empty conversation in a project, in memory only.
func (s *State) newIn(pr *preview.Project) {
	s.created++
	cv := &preview.Conversation{ID: fmt.Sprintf("%s/new-%d", pr.ID, s.created), Updated: preview.Now, Created: 10000 + s.created}
	pr.Conversations = append([]*preview.Conversation{cv}, pr.Conversations...)
	*s.opened("project/" + pr.ID) = true
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
