package shell

import (
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

const workspaceGroupSize = 28
const workspaceGroupGap = 4

type workspaceGroupReveal struct {
	shown any // the shown tab's key
	width float32
}

// rememberGroupFocus keeps a group's focused pane while another group or a
// standalone view shows. Member keys survive tree copies and split resizing;
// membership is checked again before a remembered pane receives focus.
func (a *App) rememberGroupFocus(t panes.Target) {
	w := &a.panes
	if w.Group(t) < 0 {
		return
	}
	if w.groupFocus == nil {
		w.groupFocus = map[panes.Target]panes.Target{}
	}
	for _, member := range w.Members(t) {
		w.groupFocus[member] = t
	}
}

// workspaceGroupTarget returns the pane to focus when a group is selected.
func (a *App) workspaceGroupTarget(group *panes.Node) panes.Target {
	members := group.Targets()
	w := &a.panes
	for _, member := range members {
		if remembered, ok := w.groupFocus[member]; ok && w.Group(remembered) == w.Group(member) {
			return remembered
		}
	}
	return members[0]
}

// headerTab is a tab of the title band: a split group, or a session or
// terminal standing alone. A session's tab is open until it is closed; an
// independent terminal's is kept or passing.
type headerTab struct {
	group *panes.Node // a split group's, nil for one standing alone
	index int         // the group's place in the workspace
	alone panes.Target
	// kept is a tab open until closed; a terminal's passing tab names the
	// terminal shown alone, in italics, and goes with it.
	kept, shown bool
	title       string
}

func (t headerTab) key() any {
	if t.group != nil {
		return t.group
	}
	return t.alone
}

// targets are the panes a tab shows: a group's, or the one standing alone.
func (t headerTab) targets() []panes.Target {
	if t.group != nil {
		return t.group.Targets()
	}
	return []panes.Target{t.alone}
}

// tabPlace is a tab's place in the workspace's tab order: its first target
// there, or -1 before orderTabs gives it one.
func (a *App) tabPlace(t headerTab) int {
	members := t.targets()
	return slices.IndexFunc(a.panes.order, func(o panes.Target) bool { return slices.Contains(members, o) })
}

// headerTabDrag is the value a header tab drags: the tab's key.
type headerTabDrag struct{ key any }

// moveTab puts the tab keyed source before target, or after it, by moving
// the source's entry in the tab order, which the window's preferences keep.
func (a *App) moveTab(tabs []headerTab, source any, target headerTab, after bool) {
	from := slices.IndexFunc(tabs, func(t headerTab) bool { return t.key() == source })
	if from < 0 || source == target.key() {
		return
	}
	src, dst := a.tabPlace(tabs[from]), a.tabPlace(target)
	if src < 0 || dst < 0 {
		return
	}
	w := &a.panes
	moved, anchor := w.order[src], w.order[dst]
	out := slices.Delete(slices.Clone(w.order), src, src+1)
	at := slices.Index(out, anchor)
	if after {
		at++
	}
	w.order = slices.Insert(out, at, moved)
}

// headerTabs are the conversation page's tabs, as an editor's, in the
// order they opened: the open ones, a passing one for a terminal shown alone
// otherwise, and the split groups. A session shown without having been
// opened, as a selection the window fell back to, has none. With nothing to
// show the band shows the title. An open target that joined a group shows in
// its group's tab, and one not loaded yet is left out until it is.
func (a *App) headerTabs(c *ui.Context) []headerTab {
	w := &a.panes
	if a.Page != ConversationPage {
		return nil
	}
	var kept []panes.Target
	for _, t := range w.Kept {
		if w.Group(t) < 0 && a.paneExists(t) {
			kept = append(kept, t)
		}
	}
	var alone panes.Target
	if w.Displayed() == nil && a.paneExists(w.Active) {
		alone = w.Active
	}
	tabs := make([]headerTab, 0, len(kept)+1+len(w.Groups))
	for _, t := range kept {
		tabs = append(tabs, headerTab{alone: t, kept: true, shown: t == alone, title: a.paneTitle(c, t)})
	}
	if alone.Kind == panes.Terminal && !slices.Contains(kept, alone) {
		tabs = append(tabs, headerTab{alone: alone, shown: true, title: a.paneTitle(c, alone)})
	}
	active := w.Group(w.Active)
	for i, group := range w.Groups {
		tabs = append(tabs, headerTab{group: group, index: i, shown: i == active, title: a.groupTitle(c, group)})
	}
	if len(tabs) == 0 {
		return nil
	}
	a.orderTabs(tabs)
	return tabs
}

// OpenConversation opens session id as the user asked for it, from a sidebar
// row, a link, search, a notification or another page: it shows on the
// conversation page, in its split group when it is in one, and otherwise has
// a tab until that is closed. A selection the window falls back to when data
// changes is no open and adds no tab, so closing the last tab stays closed.
func (a *App) OpenConversation(id string) {
	a.Page = ConversationPage
	if a.Sidebar.Selected != id {
		a.Sidebar.SelectOn("", id)
	}
	a.openTab(session(id))
}

// openTab gives session t a tab of its own, once: a loaded session standing
// alone. One in a split group is reached by the group's tab.
func (a *App) openTab(t panes.Target) {
	if a.Data.Find(t.ID) != nil && a.panes.Group(t) < 0 {
		a.panes.Keep(t)
	}
}

// orderTabs sorts the tabs by the order they opened in: each holds its
// place by its first target in the workspace's order, and a new one goes
// after the others. A group made of tabs takes the first one's place. The
// places of tabs gone are forgotten, so one that opens again comes last,
// except a kept tab's or a saved group's not loaded yet, which come back.
func (a *App) orderTabs(tabs []headerTab) {
	w := &a.panes
	place := a.tabPlace
	for _, t := range tabs {
		if place(t) < 0 {
			w.order = append(w.order, t.targets()[0])
		}
	}
	slices.SortStableFunc(tabs, func(x, y headerTab) int { return place(x) - place(y) })
	used := map[panes.Target]bool{}
	for _, t := range tabs {
		used[w.order[place(t)]] = true
	}
	var saved []panes.Target
	for _, g := range w.Saved() {
		saved = append(saved, g.Targets()...)
	}
	w.order = slices.DeleteFunc(w.order, func(o panes.Target) bool {
		return !used[o] && (a.paneExists(o) || !slices.Contains(w.Kept, o) && !slices.Contains(saved, o))
	})
}

// workspaceGroupWidth is the tabs' width in the room the strip leaves, each
// tab as wide as its title up to groupTabMax. More remain reachable
// by horizontal scrolling rather than covering page actions. A lone unkept
// entry uses the page title only when it is already shown.
func (a *App) workspaceGroupWidth(c *ui.Context, room float32) float32 {
	tabs := a.headerTabs(c)
	if len(tabs) == 0 || len(tabs) == 1 && tabs[0].shown && !tabs[0].kept {
		return 0
	}
	width := float32(len(tabs)-1) * (2*kit.StripGap + 1)
	for _, t := range tabs {
		w, _ := c.MeasureText(0, ui.Span{Text: t.title, Size: 14})
		width += min(w+tabChrome, groupTabMax)
	}
	return min(width, max(room, 0))
}

// groupTabMax is a tab's widest; tabChrome what a tab adds to its text:
// the padding, and its split mark, status mark or icon with the gap beside
// it, a status mark's 12 counted as an icon's 14.
const (
	groupTabMax = 220
	tabChrome   = 8 + 8 + 14 + 6 + kit.StripTabTextPadding
)

// groupTitle names a split group by its panes' titles, in order.
func (a *App) groupTitle(c *ui.Context, group *panes.Node) string {
	var titles []string
	for _, t := range group.Targets() {
		titles = append(titles, a.paneTitle(c, t))
	}
	return strings.Join(titles, " · ")
}

// workspaceGroups are the band's tabs in the strip tabs' look, a short
// divider between each two. A group's has the split mark and
// its panes' titles; one standing alone its title and a close button, a
// terminal's in italics while it only passes. Each has its system menu:
// close, a terminal's also keep or stop keeping, or undo the split. Each
// uses MyGo's button input, so presses do not drag the title bar.
func (a *App) workspaceGroups(c *ui.Context, width float32) {
	w := &a.panes
	a.rememberGroupFocus(w.Active)
	for member := range w.groupFocus {
		if w.Group(member) < 0 {
			delete(w.groupFocus, member)
		}
	}
	p := theme.Of(c)
	tabs := a.headerTabs(c)
	strip := ui.ScrollHorizontal(c.Key("workspace-groups")).Width(width).Height(HeaderHeight).Shrink(0).
		Scrollbars(false).Gap(kit.StripGap).AlignItems(ui.Center)
	var shown any
	for _, t := range tabs {
		if t.shown {
			shown = t.key()
		}
	}
	last := ui.Local(strip, "revealed", func() workspaceGroupReveal { return workspaceGroupReveal{} })
	reveal := last.shown != shown || last.width != width
	// The first build has no layout to reveal. Keep the request for the next
	// frame; after that, ordinary scrolling can leave the shown tab behind.
	if strip.LayoutBounds().W > 0 {
		*last = workspaceGroupReveal{shown: shown, width: width}
	}
	strip.Children(func() {
		for i, t := range tabs {
			if i > 0 {
				kit.StripDivider(c, "divider-"+strconv.Itoa(i))
			}
			// Shown in the strip tabs' fill, but revealed only when it
			// changes: StripTab's active tab would scroll back every frame.
			// A tab is cut to the strip, so one that alone outgrows it keeps
			// its close button in view.
			b, hovered := kit.StripTab(c, t.key(), t.title, false, min(groupTabMax, width))
			if t.shown {
				b.Background(p.TabActive).TextColor(p.TabActiveText).AccessSelected(true)
				if reveal {
					b.ScrollIntoView()
				}
			}
			b.Shrink(0)
			a.tabReorder(c, b, t, tabs)
			if t.group != nil {
				a.groupTab(c, b, t, hovered)
				continue
			}
			a.aloneTab(c, b, t, tabs, hovered)
		}
	})
}

// tabReorder makes a header tab drag into another place in the strip, as
// More's rows do: dimmed in place while the copy follows the pointer, it
// goes before the tab it is dropped on, or after it past its middle, an
// insertion line marking that edge while it is over the tab.
func (a *App) tabReorder(c *ui.Context, b ui.Element, t headerTab, tabs []headerTab) {
	b.Drag(headerTabDrag{t.key()})
	if b.Dragging() {
		b.Opacity(0.5)
	}
	x, _, in := b.PointerPosition()
	after := x >= b.Bounds().W/2
	if v, ok := ui.Drop[headerTabDrag](b); ok && in {
		a.moveTab(tabs, v.key, t, after)
	}
	if v, ok := ui.DragOver[headerTabDrag](b); ok && in && v.key != t.key() {
		primary := theme.Of(c).Primary
		b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
			at := r.X
			if after {
				at = r.X + r.W - 2
			}
			pt.Fill(ui.Rect{X: at, Y: r.Y + 4, W: 2, H: r.H - 8}, primary, 1)
		})
	}
}

// groupTab is a split group's tab: it shows the group, and its menu undoes
// the split.
func (a *App) groupTab(c *ui.Context, b ui.Element, t headerTab, hovered bool) {
	label := i18n.T(c, "pane_group", "number", strconv.Itoa(t.index+1))
	kit.StripTabBody(c, b, icons.Columns, t.title, t.shown, hovered, false, "", nil).Label(label)
	kit.Tooltip(c, b, label+": "+t.title)
	group := t.group
	b.ContextMenu(func(m *ui.Menu) {
		if m.Item(i18n.T(c, "workspace_tab_unsplit")).Chosen() {
			a.panes.Dissolve(a.panes.Group(group.Targets()[0]))
		}
	})
	b.OnClick(func() { a.showTab(c, t) })
}

// aloneTab is the tab of a session or terminal standing alone. A session's
// is open until closed; a terminal's is kept, or passing in italics until a
// double click keeps it. Closing it closes the tab, never the session's task
// or the terminal, and, while it shows, shows its neighbour.
func (a *App) aloneTab(c *ui.Context, b ui.Element, t headerTab, tabs []headerTab, hovered bool) {
	terminal := t.alone.Kind == panes.Terminal
	if terminal && !t.kept {
		b.Italic()
	}
	close := func() { a.closeHeaderTab(c, t, tabs) }
	// A session's tab leads with its status mark, as its sidebar row.
	// A terminal's leads with its mark.
	lead := func() { ui.Icon(c, icons.Terminal).FontSize(14).Shrink(0) }
	if cv := a.paneSession(t.alone); t.alone.Kind == panes.Session && cv != nil {
		lead = func() { sidebar.StatusMark(c, cv.Status) }
	}
	kit.StripTabBodyWith(c, b, lead, t.title, t.shown, hovered, i18n.T(c, "close"), close)
	kit.Tooltip(c, b, t.title)
	if terminal && b.DoubleClicked() {
		a.panes.Keep(t.alone)
	}
	b.ContextMenu(func(m *ui.Menu) {
		if terminal {
			keep := "workspace_tab_keep"
			if t.kept {
				keep = "workspace_tab_release"
			}
			if m.Item(i18n.T(c, keep)).Chosen() {
				if t.kept {
					a.panes.Release(t.alone)
				} else {
					a.panes.Keep(t.alone)
				}
			}
			m.Separator()
		}
		if m.Item(i18n.T(c, "close")).Chosen() {
			close()
		}
	})
	b.OnClick(func() { a.showTab(c, t) })
}

// showTab shows what a tab names: a group's last focused pane, or the
// session or terminal standing alone.
func (a *App) showTab(c *ui.Context, t headerTab) {
	target := t.alone
	if t.group != nil {
		target = a.workspaceGroupTarget(t.group)
	}
	a.Page = ConversationPage
	a.focusPane(target)
	// A session standing alone shows in the main column, whose focus
	// takes ⌘W; a split's pane and a terminal have their own.
	if t.group == nil && target.Kind == panes.Session {
		a.mainPane.Focus(c)
		return
	}
	a.panes.handle(target).Focus(c)
}

// standsAlone reports whether a session stands alone in the main column of
// the conversation page, with a tab of its own or as the selection the
// window fell back to. Its pane keys work either way.
func (a *App) standsAlone() bool {
	return a.Page == ConversationPage && a.panes.Displayed() == nil && a.panes.Active.Kind == panes.Session
}

// tabClosable reports whether ⌘W in the main column closes the shown tab:
// a loaded session standing alone that has one. A session shown without a
// tab leaves the key to whatever else closes, and in a split each pane takes
// it and leaves the split first.
func (a *App) tabClosable() bool {
	t := a.panes.Active
	return a.standsAlone() && slices.Contains(a.panes.Kept, t) && a.paneExists(t)
}

// ForgetSession clears the tab, tab order entry and saved split position when
// the Node confirms deletion or the window discards a local draft.
func (a *App) ForgetSession(id string) {
	t := session(id)
	w := &a.panes
	w.Forget(t)
	w.order = slices.DeleteFunc(w.order, func(o panes.Target) bool { return o == t })
	delete(w.groupFocus, t)
	for member, focused := range w.groupFocus {
		if focused == t {
			delete(w.groupFocus, member)
		}
	}
	delete(w.focus, t)
	delete(w.hosts, t)
	if a.Messages != nil {
		a.Messages.Forget(id)
	}
}

// HasDraft reports whether the window currently retains the given composer state.
func (a *App) HasDraft(state *composer.State) bool {
	if state == nil {
		return false
	}
	for _, draft := range a.drafts {
		if draft == state {
			return true
		}
	}
	return false
}

// closeShownTab is ⌘W on a session standing alone: its tab closes as its
// close button does and the next tab shows. Only the window's tab goes; the
// session runs on.
func (a *App) closeShownTab(c *ui.Context) {
	tabs := a.headerTabs(c)
	for _, t := range tabs {
		if t.shown && t.group == nil {
			a.closeHeaderTab(c, t, tabs)
			return
		}
	}
}

// closeHeaderTab closes a session's or terminal's tab, and when it is the one
// shown the next tab, or else the one before, shows. With none left the
// session stays shown under the plain title; nothing opens it again until
// the user does.
func (a *App) closeHeaderTab(c *ui.Context, t headerTab, tabs []headerTab) {
	a.panes.Release(t.alone)
	if !t.shown {
		return
	}
	at := slices.IndexFunc(tabs, func(o headerTab) bool { return o.key() == t.key() })
	switch {
	case at+1 < len(tabs):
		a.showTab(c, tabs[at+1])
	case at > 0:
		a.showTab(c, tabs[at-1])
	}
}
