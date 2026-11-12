// Package sidebar is the conversation navigation beside the rail: pinned
// conversations, chats without a project, and projects with their
// conversations as a tree. See docs/sidebar.md.
package sidebar

import (
	"fmt"
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/picker"
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
	Sample   Sample
	Metrics  *Metrics
	records  recordsState
	Selected string // conversation ID
	// Sort is the order of the projects and of the conversations in the
	// chats and in each project, as a Sort: the session order preference,
	// which the sort menu sets. New gives the sidebar its own, SortUpdated;
	// the shell points it at the setting.
	Sort *int
	// shown is set when the sidebar opens a conversation, also the selected
	// one; the shell takes it to go back to the conversation.
	shown bool
	// asked is set when New session is asked; the shell takes it to put the
	// focus in the draft's editor.
	asked   bool
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
	// Picker is the window's file and directory picker, which the project
	// editor and Open folder use; the shell builds it over the dialogs.
	Picker   *picker.State
	catalogs map[string]*picker.Catalog // the preview's sample directories, by host
	confirm  confirm
}

// confirm is the one confirmation the sidebar shows at a time: archive in
// the preview, deleting a session, or removing a project.
type confirm struct {
	open                   bool
	title, message, action string
	run                    func()
}

func New(d *viewmodel.Data, selected string) *State {
	order := int(SortUpdated)
	return &State{Data: d, Selected: selected, Sort: &order, open: map[string]*bool{}, more: map[string]bool{}, Picker: &picker.State{}}
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

// sort is the order chosen, SortUpdated for a value that is none.
func (s *State) sort() Sort {
	if *s.Sort < int(SortCreated) || *s.Sort > int(SortName) {
		return SortUpdated
	}
	return Sort(*s.Sort)
}

// sessions returns the conversations of a list that are not archived, in
// the order chosen. The list is in creation order, newest first.
func (s *State) sessions(list []*preview.Conversation) []*preview.Conversation {
	byID := map[string]*preview.Conversation{}
	var entries []Entry
	for _, cv := range list {
		if !cv.Archived {
			byID[cv.ID] = cv
			entries = append(entries, Entry{ID: cv.ID, Name: cv.Title, Updated: cv.Updated})
		}
	}
	var out []*preview.Conversation
	for _, e := range Sorted(entries, s.sort()) {
		out = append(out, byID[e.ID])
	}
	return out
}

// projects returns the projects in the order chosen. The data lists them
// oldest first; a project's last run is the latest of its conversations'.
func (s *State) projects() []*preview.Project {
	byID := map[string]*preview.Project{}
	var entries []Entry
	for _, pr := range slices.Backward(s.Data.Projects) {
		e := Entry{ID: pr.ID, Name: pr.Name}
		for _, cv := range pr.Conversations {
			if !cv.Archived && cv.Updated.After(e.Updated) {
				e.Updated = cv.Updated
			}
		}
		byID[pr.ID] = pr
		entries = append(entries, e)
	}
	var out []*preview.Project
	for _, e := range Sorted(entries, s.sort()) {
		out = append(out, byID[e.ID])
	}
	return out
}

// Order is the order of next and previous conversation: pinned, chats, then
// each project's, as the sidebar lists them, without repeats.
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
	add(s.sessions(s.Data.Unassigned))
	for _, p := range s.projects() {
		add(s.sessions(p.Conversations))
	}
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
				s.limited(c, "pinned", pinned, func(cv *preview.Conversation) {
					s.conversation(c, cv, rowPinned)
				})
			})
		}
		section(c, s, "unassigned", i18n.T(c, "sessions_unassigned"), &add{"workspace_new_session", s.NewChat}, func() {
			s.limited(c, "unassigned", s.sessions(s.Data.Unassigned), func(cv *preview.Conversation) {
				s.conversation(c, cv, rowPlain)
			})
		})
		section(c, s, "projects", i18n.T(c, "projects"), &add{"project_add", func() { s.OpenEditor(nil) }}, func() {
			ui.Column(c).Gap(2).Children(func() {
				for _, pr := range s.projects() {
					s.project(c, pr)
				}
			})
		})
	})
	metricsFooter(c, s.Metrics)
	s.dialogs(c)
}

// add is a group header's "+" button.
type add struct {
	label string // i18n key
	run   func()
}

// groupIcons are the theme's icon.group.* slots of the groups; pinned
// conversations take the icon.group.recent slot.
var groupIcons = map[string]string{"pinned": "group.recent", "unassigned": "group.unassigned", "projects": "group.projects"}

// section is a collapsible group under a muted title whose chevron shows
// while the pointer is over it. Its actions always show at the header's
// right, as the rows' buttons: the projects' sort menu, then the group's
// "+" when it has one. A theme with an icon for the group shows it before
// the title. The group grows open and closed (MyGo's collapsible motion);
// an empty one is its header alone.
func section(c *ui.Context, s *State, key, title string, plus *add, body func()) {
	p := theme.Of(c)
	ui.Column(c.Key(key)).Children(func() {
		muted := p.MutedForeground.Alpha(0.7)
		var parts ui.CollapsibleParts
		ui.Row(c).Height(32).AlignItems(ui.Center).Children(func() {
			parts = ui.CollapsibleBase(c, s.opened(key))
			hovered := parts.Trigger.Hovered()
			parts.Trigger.Grow(1).Shrink(1).MinWidth(0).Height(32).PaddingX(8).Gap(4).Radius(p.Radius).Cursor(ui.CursorPointer).
				TextColor(muted).Children(func() {
				if icon := groupIcons[key]; kit.HasArtIcon(c, icon) {
					kit.ArtIcon(c, icon, nil, 16, ui.Color{}).Margin(0, 4, 0, 0)
				}
				kit.Level(c, ui.Text(c, title).FontSize(14).SingleLine(), 14)
				chevron := ui.Icon(c, icons.ChevronDown).FontSize(12).Rotate(-90 * (1 - parts.Progress()))
				if !hovered {
					chevron.Opacity(0)
				}
			})
			if plus == nil {
				return
			}
			// The rows' buttons: 20 square, 8 apart and from the edge.
			ui.Row(c).Gap(8).PaddingEnd(8).AlignItems(ui.Center).Children(func() {
				icon := icons.Plus
				if key == "projects" {
					s.sortMenu(c, muted)
					icon = icons.FolderPlus
				}
				kit.IconButton(c.Key("add"), icon, i18n.T(c, plus.label), 20, false).TextColor(muted).OnClick(plus.run)
			})
		})
		parts.Panel(body)
	})
}

// sortMenu is the projects' sort button: a menu of the orders with a check
// on the one chosen, which also orders the conversations of the chats and
// of each project.
func (s *State) sortMenu(c *ui.Context, color ui.Color) {
	label := i18n.T(c, "sidebar_sort")
	kit.MenuButton(c.Key("sort"), "sidebar-sort", func(b ui.Element) {
		kit.IconLook(c, b, icons.ListFilter, label, "", 20, false).TextColor(color)
	}, func(m *kit.Menu) {
		for _, o := range Sorts {
			if m.Item(i18n.T(c, o.Label)).Checked(o.Sort == s.sort()).Chosen() {
				*s.Sort = int(o.Sort)
			}
		}
	}, kit.MenuOptions{CheckRight: true})
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
			kit.ProjectGlyph(c, pr.Icon, pr.Color, 14).Label(i18n.T(c, "project_icon_"+appearanceIcon(pr.Icon)))
			kit.Level(c, ui.Text(c, pr.Name).FontSize(14).SingleLine().Grow(1).Shrink(1), 14)
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
			ui.Column(c).Gap(2).Children(func() {
				s.limited(c, pr.ID, s.sessions(pr.Conversations), func(cv *preview.Conversation) {
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
	case hovered || cv.Status == preview.Running:
		text = p.SidebarForeground
	}
	b.Drag(panes.Drag{Target: panes.Target{Kind: panes.Session, ID: cv.ID}, Title: title})
	switch {
	case kind == rowPinned:
		s.reorder(c, b, cv, s.pinnedDrag, func(source string, after bool) { s.MovePin(source, cv.ID, after) })
	case kind == rowTree && s.sort() == SortCreated:
		// The manual order is the Node's, which only the creation order
		// shows.
		s.reorder(c, b, cv, func(d panes.Drag) bool { return s.sessionDrag(d, cv) },
			func(source string, after bool) { s.MoveSession(source, cv.ID, after) })
	}
	b.ContextMenu(func(m *ui.Menu) { s.sessionMenu(c, m, cv) })
	s.details(c, b, cv)
	if kit.DragChip(c, b, title) {
		return
	}
	b.TextColor(text).Children(func() {
		status(c, cv.Status, text)
		kit.Level(c, ui.Text(c, title).FontSize(14).SingleLine().Grow(1).Shrink(1), 14)
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

// reorder makes a row, which drags its session, take another row that it
// accepts to a new place before or after it, a 2 DIP line showing where it
// would go: a pinned row among the pinned, a project's row among the
// project's.
func (s *State) reorder(c *ui.Context, b ui.Element, cv *preview.Conversation, accepts func(panes.Drag) bool, move func(source string, after bool)) {
	if b.Dragging() {
		b.Opacity(0.5)
	}
	if v, over := ui.DragOver[panes.Drag](b); over && v.Target.ID != cv.ID && accepts(v) {
		_, y, _ := b.PointerPosition()
		after := y >= b.Bounds().H/2
		line := ui.Box(c.Key("drop")).Absolute().Left(0).Right(0).Height(2).Radius(1).Background(theme.Of(c).Primary).PassThrough()
		if after {
			line.Bottom(-1)
		} else {
			line.Top(-1)
		}
	}
	if v, ok := ui.Drop[panes.Drag](b); ok && v.Target.ID != cv.ID && accepts(v) {
		_, y, _ := b.PointerPosition()
		move(v.Target.ID, y >= b.Bounds().H/2)
	}
}

// sessionDrag reports whether a drag carries another session of cv's
// project that is not archived.
func (s *State) sessionDrag(d panes.Drag, cv *preview.Conversation) bool {
	source := s.Data.Find(d.Target.ID)
	if d.Target.Kind != panes.Session || source == nil || source == cv || source.Archived || source.Node != cv.Node {
		return false
	}
	pr := s.Data.ProjectOf(cv.ID)
	return pr != nil && pr == s.Data.ProjectOf(source.ID)
}

// MoveSession moves a project's session before or after another of the
// same project in its manual order.
func (s *State) MoveSession(source, target string, after bool) {
	from, to := s.Data.Find(source), s.Data.Find(target)
	pr := s.Data.ProjectOf(target)
	if from == nil || to == nil || pr == nil || pr != s.Data.ProjectOf(source) {
		return
	}
	if s.Actions != nil {
		if s.Actions.MoveSession != nil {
			s.Actions.MoveSession(from, to, after)
		}
		return
	}
	var ids []string
	byID := map[string]*preview.Conversation{}
	for _, cv := range pr.Conversations {
		ids, byID[cv.ID] = append(ids, cv.ID), cv
	}
	moved, ok := MovePin(ids, source, target, after)
	if !ok {
		return
	}
	for i, id := range moved {
		pr.Conversations[i] = byID[id]
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
// loader, in the color of the row's text and named as running, while it
// runs, otherwise the status dot in its tone. A row that does not run has
// no loader and asks for no frame.
func status(c *ui.Context, st preview.Status, color ui.Color) {
	slot := ui.Box(c).Absolute().Left(guideX-6).Top(rowHeight/2-6).Size(12, 12).Center().PassThrough()
	switch st {
	case preview.Running:
		slot.Children(func() { kit.LoaderIn(c, 12, color) })
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

// TakeDraft reports, once, whether New session was asked since the last
// call: the shell then puts the focus in the draft's editor, also when the
// page already shows that draft and nothing else changes.
func (s *State) TakeDraft() bool {
	asked := s.asked
	s.asked = false
	return asked
}

// unsent reports whether a conversation is a new one nothing was sent in:
// the reference's single draft. Live, the runtime owns the drafts and keeps
// them out of the sidebar's data, so none of its rows is one.
func (s *State) unsent(cv *preview.Conversation) bool {
	return s.Actions == nil && cv.Title == "" && len(cv.Turns) == 0 && cv.Status == preview.Idle && !cv.Archived
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

// NewChat opens an empty chat without a project, in memory only: the unsent
// one when there is one, as the reference keeps a single draft, otherwise a
// new one. The button stays enabled over the draft already shown: its click
// puts the focus back in the draft's editor (TakeDraft).
func (s *State) NewChat() {
	s.asked = true
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
	s.asked = true
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

// NewSession opens an empty conversation in a project, as the project
// menu's New session does.
func (s *State) NewSession(pr *preview.Project) { s.newIn(pr) }

// Title is a conversation's title, or the "New conversation" copy while it
// has none.
func Title(c *ui.Context, cv *preview.Conversation) string {
	if cv.Title == "" {
		return i18n.T(c, "conversation")
	}
	return cv.Title
}
