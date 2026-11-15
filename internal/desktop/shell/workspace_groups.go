package shell

import (
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
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

// workspaceGroupWidth reserves a bounded part of the title band. More groups
// remain reachable by horizontal scrolling rather than covering page actions.
func (a *App) workspaceGroupWidth(room float32) float32 {
	n := len(a.panes.Groups)
	if n == 0 || n == 1 && a.panes.Group(a.panes.Active) >= 0 {
		return 0
	}
	width := float32(n*workspaceGroupSize + (n-1)*workspaceGroupGap)
	return min(width, max(room-ButtonGap, 0), max(workspaceGroupSize, min(220, room*0.45)))
}

// workspaceGroups are the split groups' numbered buttons beside the title.
// Each button uses MyGo's button input so presses do not drag the title bar.
func (a *App) workspaceGroups(c *ui.Context, width float32) {
	w := &a.panes
	a.rememberGroupFocus(w.Active)
	for member := range w.groupFocus {
		if w.Group(member) < 0 {
			delete(w.groupFocus, member)
		}
	}
	active := w.Group(w.Active)
	strip := ui.ScrollHorizontal(c.Key("workspace-groups")).Width(width).Height(HeaderHeight).Shrink(0).
		Scrollbars(false).Gap(workspaceGroupGap).AlignItems(ui.Center)
	var shown *panes.Node
	if active >= 0 {
		shown = w.Groups[active]
	}
	last := ui.Local(strip, "revealed", func() workspaceGroupReveal { return workspaceGroupReveal{} })
	reveal := last.group != shown || last.width != width
	// The first build has no layout to reveal. Keep the request for the next
	// frame; after that, ordinary scrolling can leave the active number behind.
	if strip.LayoutBounds().W > 0 {
		*last = workspaceGroupReveal{group: shown, width: width}
	}
	strip.Children(func() {
		for i, group := range w.Groups {
			number := strconv.Itoa(i + 1)
			label := i18n.T(c, "pane_group", "number", number)
			var b ui.Element
			if i == active {
				b = kit.Primary(c.Key(group), number, false).Children(func() {
					ui.Text(c, number).FontSize(14).SingleLine()
				})
				if reveal {
					b.ScrollIntoView()
				}
			} else {
				b = kit.Secondary(c.Key(group), number)
			}
			b.Label(label).Size(workspaceGroupSize, workspaceGroupSize).PaddingX(0).Shrink(0).AccessSelected(i == active)
			kit.Tooltip(c, b, label)
			b.OnClick(func() {
				target := a.workspaceGroupTarget(group)
				a.Page = ConversationPage
				a.focusPane(target)
				w.handle(target).Focus(c)
			})
		}
	})
}
