package shell

import (
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
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
// terminal standing alone, kept or passing.
type headerTab struct {
	group *panes.Node // a split group's, nil for one standing alone
	index int         // the group's place in the workspace
	alone panes.Target
	// kept is a tab kept until closed; a passing tab names what shows
	// alone, in italics, and goes with it.
	kept, shown bool
	title       string
}

func (t headerTab) key() any {
	if t.group != nil {
		return t.group
	}
	return t.alone
}

// headerTabs are the conversation page's tabs, as an editor's: the kept
// ones, then a passing one for what shows alone otherwise, then the split
// groups. With nothing to show the band shows the title. A kept target
// that joined a group shows in its group's tab, and one not loaded yet is
// left out until it is.
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
	if alone != (panes.Target{}) && !slices.Contains(kept, alone) {
		tabs = append(tabs, headerTab{alone: alone, shown: true, title: a.paneTitle(c, alone)})
	}
	active := w.Group(w.Active)
	for i, group := range w.Groups {
		tabs = append(tabs, headerTab{group: group, index: i, shown: i == active, title: a.groupTitle(c, group)})
	}
	if len(tabs) == 0 {
		return nil
	}
	return tabs
}

// workspaceGroupWidth is the tabs' width in the room the strip leaves, each
// tab as wide as its title up to groupTabMax. More remain reachable
// by horizontal scrolling rather than covering page actions. A shown split
// keeps its own tab: the tab, with its split mark, is what tells a split
// from a conversation shown alone.
func (a *App) workspaceGroupWidth(c *ui.Context, room float32) float32 {
	tabs := a.headerTabs(c)
	if len(tabs) == 0 {
		return 0
	}
	width := float32(len(tabs)-1) * (2*kit.StripGap + 1)
	for _, t := range tabs {
		w, _ := c.MeasureText(0, ui.Span{Text: t.title, Size: 14})
		chrome := float32(groupTabChrome)
		if t.group == nil {
			chrome += closeChrome - 14 - 6
		}
		width += min(w+chrome, groupTabMax)
	}
	return min(width, max(room, 0))
}

// groupTabMax is a tab's widest; groupTabChrome what a group tab adds to
// its text: the padding, the split mark and the gap beside it, and
// closeChrome what a tab standing alone adds for its close button.
const (
	groupTabMax    = 220
	groupTabChrome = 8 + 8 + 14 + 6
	closeChrome    = 6 + 20 - 4
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
// divider between two that are not shown. A group's has the split mark and
// its panes' titles; one standing alone its title and a close button, in
// italics while it only passes. A double click keeps a passing tab, and
// each has its system menu: keep or stop keeping and close, or undo the
// split. Each uses MyGo's button input, so presses do not drag the title
// bar.
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
				kit.StripDivider(c, "divider-"+strconv.Itoa(i), !t.shown && !tabs[i-1].shown)
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
			if t.group != nil {
				a.groupTab(c, b, t, hovered)
				continue
			}
			a.aloneTab(c, b, t, tabs, hovered)
		}
	})
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

// aloneTab is the tab of a session or terminal standing alone: kept, or
// passing in italics until a double click keeps it. Closing it stops
// keeping it and, while it shows, shows its neighbour.
func (a *App) aloneTab(c *ui.Context, b ui.Element, t headerTab, tabs []headerTab, hovered bool) {
	if !t.kept {
		b.Italic()
	}
	close := func() { a.closeHeaderTab(c, t, tabs) }
	kit.StripTabBody(c, b, nil, t.title, t.shown, hovered, false, i18n.T(c, "close"), close)
	kit.Tooltip(c, b, t.title)
	if b.DoubleClicked() {
		a.panes.Keep(t.alone)
	}
	b.ContextMenu(func(m *ui.Menu) {
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
	a.panes.handle(target).Focus(c)
}

// closeHeaderTab closes a session's or terminal's tab: it is kept no more, and
// when it is the one shown the next tab, or else the one before, shows.
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
