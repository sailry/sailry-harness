// Package sidebar is the conversation navigation beside the rail: pinned
// conversations, chats without a project, projects with their conversations
// as a tree, and recent conversations. See docs/sidebar.md.
package sidebar

import (
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// Limit is how many conversations a group shows before "Show more".
const Limit = Initial

// Layout metrics.
const (
	rowHeight = 28
	indent    = 30 // conversation rows under a project
	guideX    = 16 // the status slot's center, from the row's left edge
)

// State is the sidebar's part of the app state.
type State struct {
	Data     *viewmodel.Data
	Actions  *Actions
	Metrics  *Metrics
	records  recordsState
	Selected string // conversation ID
	// Showing is whether the page shown is the selected conversation, which
	// the shell sets each frame. Over another page (a host, a project) the
	// sidebar's rows and New session lead back to the conversation.
	Showing bool
	// shown is set when the sidebar opens a conversation, also the selected
	// one; the shell takes it to go back to the conversation.
	shown   bool
	open    map[string]*bool
	more    map[string]bool
	created int

	renaming         string // the conversation renamed in place
	draft            string // its title as typed
	drawn            bool   // its input is built, in its first row, this frame
	renameTarget     *viewmodel.Conversation
	renamePending    bool
	renameGeneration uint64

	editor           Editor
	editorGeneration uint64
	confirm          confirm
}

// confirm is the one confirmation the sidebar shows at a time: archive in
// the preview, deleting a session, or removing a project.
type confirm struct {
	open                   bool
	title, message, action string
	run                    func()
}

func New(d *viewmodel.Data, selected string) *State {
	s := &State{Data: d, Selected: selected, open: map[string]*bool{}, more: map[string]bool{}}
	// The recent group starts closed.
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
		b := kit.Primary(c.Key("new"), i18n.T(c, "conversation"), s.AtDraft()).FillWidth()
		kit.NewSessionArt(c, b).Children(func() {
			ui.Icon(c, icons.Plus).FontSize(16)
			ui.Text(c, i18n.T(c, "conversation")).FontSize(14).SingleLine()
		}).OnClick(s.NewChat)
	})
	ui.Scroll(c.Key("sessions")).Grow(1).Padding(8, 12).Gap(4).Children(func() {
		if pinned := s.pinned(); len(pinned) > 0 {
			section(c, s, "pinned", i18n.T(c, "sidebar_pinned"), nil, func() {
				s.limited(c, "pinned", pinned, func(cv *preview.Conversation) {
					s.conversation(c, cv, rowPinned)
				})
			})
		}
		section(c, s, "unassigned", i18n.T(c, "sessions_unassigned"), &add{"workspace_new_session", s.NewChat, s.AtDraft()}, func() {
			list := listed(s.Data.Unassigned)
			if len(list) == 0 {
				empty(c, "project_sessions_empty")
				return
			}
			s.limited(c, "unassigned", list, func(cv *preview.Conversation) {
				s.conversation(c, cv, rowPlain)
			})
		})
		section(c, s, "projects", i18n.T(c, "projects"), &add{"project_add", func() { s.OpenEditor(nil) }, false}, func() {
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
				s.conversation(c.Key("recent"), cv, rowRecent)
			}
		})
	})
	metricsFooter(c, s.Metrics)
	s.dialogs(c)
}

func empty(c *ui.Context, key string) {
	ui.Text(c, i18n.T(c, key)).FontSize(14).Padding(4, 8, 4, indent).TextColor(theme.Of(c).MutedForeground.Alpha(0.7))
}

// add is a group header's "+" button.
type add struct {
	label    string // i18n key
	run      func()
	disabled bool
}

// groupIcons are the theme's icon.group.* slots of the groups; pinned
// conversations take the recent group's.
var groupIcons = map[string]string{"pinned": "group.recent", "unassigned": "group.unassigned", "projects": "group.projects", "recent": "group.recent"}

// section is a collapsible group under a muted title whose chevron, and "+"
// when it has one, show while the pointer is over it. A theme with an icon
// for the group shows it before the title. The group grows open and closed
// (MyGo's collapsible motion).
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
					Margin(0, 4, 0, 0).Disabled(plus.disabled).OnClick(plus.run)
				if !hovered && !b.Hovered() && !b.FocusVisible() {
					b.Opacity(0)
				}
			}
		})
		parts.Panel(body)
	})
}

// limited shows the first Limit conversations of a list and a toggle for the
// rest.
func (s *State) limited(c *ui.Context, key string, list []*preview.Conversation, row func(cv *preview.Conversation)) {
	n, more := Window(len(list), s.more[key])
	for i := range n {
		row(list[i])
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

// project is a project row and, while it is open, its conversations hanging
// from the tree guide. They grow in and out as it opens and closes. Its
// right-click menu is the system's.
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
				s.limited(c, pr.ID, list, func(cv *preview.Conversation) {
					s.conversation(c, cv, rowTree)
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

// conversation is a session row: its status, title and age, and on hover a
// pin and an archive button. A double click renames it in place. Its right-
// click menu is the system's; the pointer resting on it shows its details in
// a card. It drags onto the workspace to open in a split, wearing the drag
// preview's look.
func (s *State) conversation(c *ui.Context, cv *preview.Conversation, kind rowKind) {
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
	b.Drag(panes.Drag{Target: panes.Target{Kind: panes.Session, ID: cv.ID}, Title: title})
	if kind == rowPinned {
		s.reorder(c, b, cv)
	}
	b.ContextMenu(func(m *ui.Menu) { s.sessionMenu(c, m, cv) })
	s.details(c, b, cv)
	if kit.DragChip(c, b, title) {
		return
	}
	b.TextColor(text).Children(func() {
		if kind != rowRecent {
			status(c, cv.Status)
		}
		ui.Text(c, title).FontSize(14).SingleLine().Grow(1).Shrink(1)
		if s.nonMainWorktree(cv) {
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
			now := preview.Now
			if s.Actions != nil {
				now = c.Now()
			}
			ui.Text(c, ageAt(c, cv.Updated, now)).FontSize(12).SingleLine().TextColor(p.MutedForeground.Alpha(0.6))
		}
	}).OnClick(func() { s.show(cv.ID) })
	if b.DoubleClicked() {
		s.StartRename(cv)
	}
}

func (s *State) nonMainWorktree(cv *preview.Conversation) bool {
	if project := s.Data.ProjectOf(cv.ID); project != nil {
		for _, tree := range project.Worktrees {
			if cv.WorktreePath != "" && cv.WorktreePath == tree.Path || cv.WorktreePath == "" && cv.Worktree != "" && cv.Worktree == tree.Branch {
				return !tree.Main
			}
		}
	}
	return false
}

// reorder makes a pinned row, which drags its session, take another pinned
// one to a new place among the pinned, a 2 DIP line showing where it would
// go.
func (s *State) reorder(c *ui.Context, b ui.Element, cv *preview.Conversation) {
	if b.Dragging() {
		b.Opacity(0.5)
	}
	if v, over := ui.DragOver[panes.Drag](b); over && v.Target.ID != cv.ID && s.pinnedDrag(v) {
		_, y, _ := b.PointerPosition()
		after := y >= b.Bounds().H/2
		line := ui.Box(c.Key("drop")).Absolute().Left(0).Right(0).Height(2).Radius(1).Background(theme.Of(c).Primary).PassThrough()
		if after {
			line.Bottom(-1)
		} else {
			line.Top(-1)
		}
	}
	if v, ok := ui.Drop[panes.Drag](b); ok && s.pinnedDrag(v) {
		_, y, _ := b.PointerPosition()
		s.MovePin(v.Target.ID, cv.ID, y >= b.Bounds().H/2)
	}
}

// pinnedDrag reports whether a drag carries a pinned session.
func (s *State) pinnedDrag(d panes.Drag) bool {
	cv := s.Data.Find(d.Target.ID)
	return d.Target.Kind == panes.Session && cv != nil && cv.Pinned
}

// MovePin moves a pinned conversation before or after another, giving the
// pins their new order.
func (s *State) MovePin(source, target string, after bool) {
	if s.Actions != nil && s.Actions.MovePin != nil {
		s.Actions.MovePin(source, target, after)
		return
	}
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
	if s.Actions != nil && s.Actions.Pin != nil {
		s.Actions.Pin(cv)
		return
	}
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

// details shows the hover card of a session row: its title, then its project,
// branch and worktree path, right of the row and centered on it
// (Anchor::LeftCenter).
func (s *State) details(c *ui.Context, row ui.Element, cv *preview.Conversation) {
	d := kit.Session{Title: Title(c, cv)}
	if pr := s.Data.ProjectOf(cv.ID); pr != nil {
		d.Project, d.Branch, d.Path = pr.Name, pr.Branch, pr.Path
		for _, w := range pr.Worktrees {
			if cv.Worktree != "" && w.Branch == cv.Worktree {
				d.Branch, d.Path = w.Branch, w.Path
			}
		}
		if cv.WorktreePath != "" {
			d.Path = cv.WorktreePath
		}
	}
	kit.SessionCard(c, row, d, true, kit.Right, func(card ui.Element) {
		card.AttachTo(row, ui.AnchorRight, ui.AnchorLeft).MarginStart(8)
	})
}

// status puts the conversation's state in the 12x12 slot on the elbow: the
// loader while it runs, otherwise the status dot in its tone.
func status(c *ui.Context, st preview.Status) {
	slot := ui.Box(c).Absolute().Left(guideX-6).Top(rowHeight/2-6).Size(12, 12).Center().PassThrough()
	switch st {
	case preview.Running:
		slot.Label(i18n.T(c, "activity_running")).Children(func() { kit.Loader(c, 12) })
	case preview.Waiting:
		slot.Children(func() { kit.StatusDot(c, kit.Caution, i18n.T(c, "activity_waiting")) })
	case preview.Unread:
		slot.Children(func() { kit.StatusDot(c, kit.Info, i18n.T(c, "notification_unread")) })
	}
}

// Age is the short age of sidebar rows: 45s, 12m, 3h, 2d.
func Age(c *ui.Context, t time.Time) string {
	return ageAt(c, t, preview.Now)
}

func ageAt(c *ui.Context, t, now time.Time) string {
	if t.IsZero() {
		return ""
	}
	d := max(now.Sub(t), 0)
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

// show opens a conversation from the sidebar: it selects it and, for the
// shell, marks that the conversation is to show, also when it is the
// selected one behind a host or project page.
func (s *State) show(id string) {
	s.Selected, s.shown = id, true
}

// TakeShown reports, once, whether the sidebar opened a conversation since
// the last call.
func (s *State) TakeShown() bool {
	shown := s.shown
	s.shown = false
	return shown
}

// unsent reports whether a conversation is a new one nothing was sent in:
// the reference's single draft. A live draft is the runtime's, under its
// "draft/" ID; a saved session without a title is not one.
func (s *State) unsent(cv *preview.Conversation) bool {
	if cv.Title != "" || len(cv.Turns) != 0 || cv.Status != preview.Idle || cv.Archived {
		return false
	}
	return s.Actions == nil || strings.HasPrefix(cv.ID, "draft/")
}

// unsentIn returns the unsent conversation of a list, nil for none.
func (s *State) unsentIn(list []*preview.Conversation) *preview.Conversation {
	for _, cv := range list {
		if s.unsent(cv) {
			return cv
		}
	}
	return nil
}

// AtDraft reports whether the page shows the unsent chat New session would
// open. New session cannot act then, so its buttons are disabled.
func (s *State) AtDraft() bool {
	cv := s.unsentIn(s.Data.Unassigned)
	return s.Showing && cv != nil && cv.ID == s.Selected
}

// NewChat opens an empty chat without a project, in memory only: the unsent
// one when there is one, as the reference keeps a single draft, otherwise a
// new one.
func (s *State) NewChat() {
	if cv := s.unsentIn(s.Data.Unassigned); cv != nil {
		s.show(cv.ID)
		return
	}
	s.shown = true
	if s.Actions != nil && s.Actions.NewDraft != nil {
		s.Actions.NewDraft(nil)
		return
	}
	s.created++
	cv := &preview.Conversation{ID: fmt.Sprintf("chat/new-%d", s.created), Updated: preview.Now, Created: 10000 + s.created}
	s.Data.Unassigned = append([]*preview.Conversation{cv}, s.Data.Unassigned...)
	s.Selected = cv.ID
}

// newIn opens an empty conversation in a project, in memory only: its
// unsent one when it has one, otherwise a new one.
func (s *State) newIn(pr *preview.Project) {
	if cv := s.unsentIn(pr.Conversations); cv != nil {
		*s.opened("project/" + pr.ID) = true
		s.show(cv.ID)
		return
	}
	s.shown = true
	if s.Actions != nil && s.Actions.NewDraft != nil {
		s.Actions.NewDraft(pr)
		return
	}
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
