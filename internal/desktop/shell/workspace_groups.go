package shell

import (
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
	group *panes.Node
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

// workspaceGroupWidth reserves a bounded part of the title band for the
// group tabs, each as wide as its titles up to groupTabMax. More groups
// remain reachable by horizontal scrolling rather than covering page actions.
// A shown split keeps its own tab: the tab, with its split mark, is what
// tells a split from a conversation shown alone.
func (a *App) workspaceGroupWidth(c *ui.Context, room float32) float32 {
	n := len(a.panes.Groups)
	if n == 0 {
		return 0
	}
	width := float32(n-1) * (2*kit.StripGap + 1)
	for _, group := range a.panes.Groups {
		w, _ := c.MeasureText(0, ui.Span{Text: a.groupTitle(c, group), Size: 14})
		width += min(w+groupTabChrome, groupTabMax)
	}
	return min(width, max(room-ButtonGap, 0), max(groupTabMax, room*0.6))
}

// groupTabMax is a group tab's widest; groupTabChrome what a tab adds to
// its text: the padding, the split mark and the gap beside it.
const (
	groupTabMax    = 220
	groupTabChrome = 8 + 4 + 14 + 6
)

// groupTitle names a split group by its panes' titles, in order.
func (a *App) groupTitle(c *ui.Context, group *panes.Node) string {
	var titles []string
	for _, t := range group.Targets() {
		titles = append(titles, a.paneTitle(c, t))
	}
	return strings.Join(titles, " · ")
}

// workspaceGroups are the split groups' tabs beside the title, in the strip
// tabs' look: the split mark and the panes' titles, a short divider between
// tabs that are not shown. Each uses MyGo's button input, so presses do not
// drag the title bar.
func (a *App) workspaceGroups(c *ui.Context, width float32) {
	w := &a.panes
	a.rememberGroupFocus(w.Active)
	for member := range w.groupFocus {
		if w.Group(member) < 0 {
			delete(w.groupFocus, member)
		}
	}
	p := theme.Of(c)
	active := w.Group(w.Active)
	strip := ui.ScrollHorizontal(c.Key("workspace-groups")).Width(width).Height(HeaderHeight).Shrink(0).
		Scrollbars(false).Gap(kit.StripGap).AlignItems(ui.Center)
	var shown *panes.Node
	if active >= 0 {
		shown = w.Groups[active]
	}
	last := ui.Local(strip, "revealed", func() workspaceGroupReveal { return workspaceGroupReveal{} })
	reveal := last.group != shown || last.width != width
	// The first build has no layout to reveal. Keep the request for the next
	// frame; after that, ordinary scrolling can leave the active tab behind.
	if strip.LayoutBounds().W > 0 {
		*last = workspaceGroupReveal{group: shown, width: width}
	}
	strip.Children(func() {
		for i, group := range w.Groups {
			if i > 0 {
				kit.StripDivider(c, "divider-"+strconv.Itoa(i), i != active && i-1 != active)
			}
			title := a.groupTitle(c, group)
			label := i18n.T(c, "pane_group", "number", strconv.Itoa(i+1))
			// Active in the strip tabs' fill, but revealed only when it
			// changes: StripTab's active tab would scroll back every frame.
			b, hovered := kit.StripTab(c, group, title, false, groupTabMax)
			if i == active {
				b.Background(p.TabActive).TextColor(p.TabActiveText).AccessSelected(true)
				if reveal {
					b.ScrollIntoView()
				}
			}
			kit.StripTabBody(c, b.Shrink(0), icons.Columns, title, i == active, hovered, false, "", nil).Label(label)
			kit.Tooltip(c, b, label+": "+title)
			b.OnClick(func() {
				target := a.workspaceGroupTarget(group)
				a.Page = ConversationPage
				a.focusPane(target)
				w.handle(target).Focus(c)
			})
		}
	})
}
