package panel

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The side list's width beside its document: 220 at first, dragged between
// sideMin and sideMax, the main area keeping sideMin.
const (
	sideWidth = 220
	sideMin   = 180
	sideMax   = 480
	handle    = 6
)

// One rule lays out every list and its document, Files and Git, in the
// panel's tabs and on the rail's pages alike.
const (
	// stackBelow is the host width under which the list goes above the
	// document: the list's first width and a 500 DIP document no longer fit
	// side by side. The panel's first width (560) stacks.
	stackBelow = sideWidth + 500
	// stackSlack is how much wider a stacked host gets before it goes back
	// side by side, so the pointer resting at the threshold flips nothing.
	stackSlack = 8
	// diffSplitMin is the least document width that shows a diff side by
	// side: two columns of 320, about forty characters each past the gutter.
	diffSplitMin = 640
	// ListShare is the stacked list's first share of the height; a drag
	// keeps it between listShareMin and listShareMax.
	ListShare    = 0.38
	listShareMin = 0.2
	listShareMax = 0.7
	// A stacked list keeps its bars and three rows under a drag.
	filesLeast = barHeight + 3*rowHeight
	gitLeast   = 3*barHeight + 3*rowHeight
)

// hostLayout is what a host, the panel's tabs or a page, was laid out as
// last: its width, and whether its list is above its document. A tab's own
// box is new with every tab, so the host's facts are the window's.
type hostLayout struct {
	width          float32
	known, stacked bool
}

type hostKey struct{ page bool }

// layoutOf decides a host's layout from the width box was laid out at. A
// host never laid out guesses: the panel stacks, as at its first width, and
// a page follows the window.
func layoutOf(c *ui.Context, page bool, box ui.Element) *hostLayout {
	h := ui.Local(c.Root(), hostKey{page}, func() hostLayout {
		w, _ := c.Size()
		return hostLayout{stacked: !page || w < stackBelow}
	})
	w := box.LayoutBounds().W
	if w <= 0 {
		return h
	}
	switch {
	case !h.known:
		h.stacked = w < stackBelow
	case h.stacked:
		h.stacked = w < stackBelow+stackSlack
	default:
		h.stacked = w < stackBelow
	}
	h.width, h.known = w, true
	return h
}

// listShare is a window's share of the height for a stacked list: its
// owner's (BindListShare) or, in a window without one, its own.
type listShare struct {
	bound *float32
	own   float32
}

type listShareKey struct{}

// BindListShare makes share the window's stacked list height, a share of
// the host's, which its owner keeps across launches; it runs before the
// window's panel and pages are built. 0 is ListShare.
func BindListShare(c *ui.Context, share *float32) {
	ui.Local(c.Root(), listShareKey{}, func() listShare { return listShare{} }).bound = share
}

func shareOf(c *ui.Context) *float32 {
	v := ui.Local(c.Root(), listShareKey{}, func() listShare { return listShare{} })
	s := &v.own
	if v.bound != nil {
		s = v.bound
	}
	if *s <= 0 {
		*s = ListShare
	}
	*s = min(max(*s, listShareMin), listShareMax)
	return s
}

// pair lays out a list and its document in a host. With room they sit side
// by side: the main area, then the list at its own width past a drag
// handle. Custom, not MyGo's Split: that sizes its first pane, so keeping
// the list's width meant converting it from the last frame's layout, and a
// first frame (a page switched to, a tab chosen, the panel opening) drew
// the main area at its minimum. Under stackBelow they stack: the list on
// top, as high as its rows up to the window's share of the height, then the
// same handle and the document in the rest. least is the list's least
// height under a drag: its bars and a few rows.
func (pr *Project) pair(c *ui.Context, key string, page bool, side *float32, least float32, main, list func()) {
	box := ui.Box(c.Key(key)).Grow(1).MinHeight(0).MinWidth(0).AlignItems(ui.Stretch).Clip()
	h := layoutOf(c, page, box)
	pr.stacked = h.stacked
	room := h.width
	if !h.stacked {
		room -= *side
	}
	pr.narrow = h.known && room < diffSplitMin || !h.known && h.stacked
	if !h.stacked {
		box.Row().Children(func() {
			ui.Column(c.Key("main")).Grow(1).Shrink(1).MinWidth(0).MinHeight(0).Children(main)
			divider(c, side, h.width)
			ui.Column(c.Key("side")).Width(*side).Shrink(0).MinHeight(0).Children(list)
		})
		return
	}
	share := shareOf(c)
	height := box.LayoutBounds().H
	box.Column().Children(func() {
		top := ui.Column(c.Key("side")).Shrink(0).MinHeight(0).MaxHeightPercent(*share * 100)
		top.Children(list)
		grip(c, share, top.LayoutBounds().H, height, least)
		ui.Column(c.Key("main")).Grow(1).Shrink(1).MinWidth(0).MinHeight(0).Children(main)
	})
}

// divider is the split's 1 DIP line under a 6 DIP drag handle: dragging
// left widens the list.
func divider(c *ui.Context, side *float32, w float32) {
	p := theme.Of(c)
	h := ui.Box(c.Key("divider")).Width(handle).Margin(0, -handle/2).ZIndex(1).Cursor(ui.CursorResizeEW).Shrink(0).
		Row().Justify(ui.Center).Children(func() {
		ui.Box(c).Width(1).FillHeight().Background(p.Border)
	})
	if h.Hovered() || h.Pressed() {
		h.Background(p.Ring.Alpha(0.5))
	}
	if dx, _, ok := h.Dragged(); ok {
		limit := float32(sideMax)
		if w > 0 {
			limit = min(limit, max(w-sideMin, sideMin))
		}
		*side = min(max(*side-dx, sideMin), limit)
	}
}

// grip is divider between a stacked list and its document: dragging down
// gives the list more of the height, from the height it has (a list of few
// rows is no higher than they are), and a double click puts the first share
// back.
func grip(c *ui.Context, share *float32, list, height, least float32) {
	p := theme.Of(c)
	h := ui.Box(c.Key("divider")).Height(handle).Margin(-handle/2, 0).ZIndex(1).Cursor(ui.CursorResizeNS).Shrink(0).
		Role(ui.RoleSplitter).Column().Justify(ui.Center).Children(func() {
		ui.Box(c).Height(1).FillWidth().Background(p.Border)
	})
	if h.Hovered() || h.Pressed() {
		h.Background(p.Ring.Alpha(0.5))
	}
	if _, dy, ok := h.Dragged(); ok && dy != 0 && height > 0 {
		lo := min(max(listShareMin, least/height), listShareMax)
		*share = min(max((list+dy)/height, lo), listShareMax)
	}
	if h.DoubleClicked() {
		*share = ListShare
	}
}
