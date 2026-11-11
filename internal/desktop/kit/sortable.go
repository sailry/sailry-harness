// SortableList is adapted from Lobe MyGo's layout.SortableList
// (github.com/lobehub/lobe-mygo pkg/layout/sortablelist.go at 5b3ecde):
//
// MIT License
//
// Copyright (c) 2026 LobeHub
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
// TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
// SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//
// Changes: the borderless variant only (rows bring their own look), the
// handle a Sailry icon button labelled by the caller, the ease inlined,
// and OnMove telling the caller of a drop that moved a row; Divided rows
// for a settings card and Disabled handles.

package kit

import (
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// SortableListProps are a SortableList's options.
type SortableListProps struct {
	// Gap is the space between rows: 8 DIPs when zero.
	Gap float32
	// Divided lays the rows against each other with a divider between
	// them, as the rows of a settings Card; Gap is not used.
	Divided bool
	// Disabled dims the handles and takes no drag or key: the order is
	// fixed for now, and the rows keep their places.
	Disabled bool
	// Handle names the drag handle for assistive technology.
	Handle string
	// OnMove runs after a drop moved the row at from to to, with *items
	// in the new order.
	OnMove func(from, to int)
}

type sortState struct {
	active, over int
	lifted       string // the key of the row at active
	keyboard     bool
	dy           float32
	bounds       []ui.Rect
	reset        bool
	dropping     string
	dropTop      float32
}

// sortLift is how far a divided row's lifted panel reaches past its place
// on each side.
const sortLift = 8

var (
	sortShift  = ui.Animation{Duration: 250 * time.Millisecond, Ease: EaseOut}
	sortSettle = ui.Animation{Duration: 250 * time.Millisecond, Ease: EaseOut}
)

// SortableList shows *items as rows that drag into a new order by the
// handle each row builds: row gets an item and a func building its handle
// where it goes (a row that cannot move builds none). key names an item
// for as long as it lives. The other rows make room as a row is dragged,
// and it settles in its place on the drop. The handle reorders from the
// keyboard too: Space or Enter picks the row up and drops it, the arrow
// keys move it, Escape puts it back.
func SortableList[T any](c *ui.Context, items *[]T, key func(T) string, o SortableListProps, row func(item T, handle func())) ui.Element {
	gap := o.Gap
	if gap <= 0 {
		gap = 8
	}
	if o.Divided {
		gap = 0
	}
	list := ui.Column(c).Gap(gap).AlignSelf(ui.Stretch)
	if o.Divided {
		list.Dividers(1, theme.Of(c).Border)
	}
	st := ui.Local(list, "sortable", func() sortState { return sortState{active: -1} })
	n := len(*items)
	if o.Disabled {
		st.active, st.keyboard, st.dy = -1, false, 0
	}
	if st.active >= 0 {
		// The caller may have changed the list under the lifted row: it is
		// followed by its key with its target as far from it as before, or
		// put back when it is gone, so no index outlives the list it was
		// taken from.
		at := slices.IndexFunc(*items, func(it T) bool { return key(it) == st.lifted })
		if at < 0 {
			st.active, st.keyboard, st.dy = -1, false, 0
		} else {
			st.active, st.over = at, min(max(st.over+at-st.active, 0), n-1)
		}
	}
	if st.active >= 0 && !st.keyboard && len(st.bounds) == n {
		st.over = sortOver(st.bounds, st.active, st.dy)
	}
	reset := st.reset
	st.reset = false
	bounds := make([]ui.Rect, n)
	released := false
	list.Children(func() {
		for i, it := range *items {
			k := key(it)
			r := ui.Row(c.Key(k)).AlignItems(ui.Center).FillWidth().MinWidth(0)
			shift := sortShiftOf(st, i, gap)
			if reset {
				r.Translate(0, r.Jump("shift", 0))
			} else {
				r.Translate(0, r.AnimateTo("shift", shift, sortShift))
			}
			if i == st.active || k == st.dropping {
				r.Opacity(0.4)
			}
			bounds[i] = r.LayoutBounds()
			r.Children(func() {
				row(it, func() {
					if sortHandle(c, st, i, n, k, o.Handle, o.Disabled) {
						released = true
					}
				})
			})
		}
	})
	st.bounds = bounds
	sortOverlay(c, list, st, items, key, row, o.Divided)
	if released {
		sortDrop(c, st, items, key, o.OnMove)
	}
	return list
}

func sortOver(bounds []ui.Rect, active int, dy float32) int {
	center := bounds[active].Y + bounds[active].H/2 + dy
	best := active
	for i, b := range bounds {
		if abs32(b.Y+b.H/2-center) < abs32(bounds[best].Y+bounds[best].H/2-center) {
			best = i
		}
	}
	return best
}

func abs32(v float32) float32 {
	if v < 0 {
		return -v
	}
	return v
}

func sortSlotTop(st *sortState) float32 {
	a, o := st.bounds[st.active], st.bounds[st.over]
	if st.over > st.active {
		return o.Y + o.H - a.H
	}
	return o.Y
}

func sortShiftOf(st *sortState, i int, gap float32) float32 {
	if st.active < 0 || len(st.bounds) <= max(st.active, st.over, i) {
		return 0
	}
	room := st.bounds[st.active].H + gap
	switch {
	case i == st.active:
		return sortSlotTop(st) - st.bounds[st.active].Y
	case st.active < i && i <= st.over:
		return -room
	case st.over <= i && i < st.active:
		return room
	}
	return 0
}

// sortHandle is a row's handle; it reports a drop.
func sortHandle(c *ui.Context, st *sortState, i, n int, key, label string, disabled bool) bool {
	h := IconButton(c.Key("sort-handle"), icons.EllipsisVertical, label, 24, false).Radius(12)
	if disabled {
		h.Disabled(true)
		return false
	}
	h.Cursor(ui.CursorGrab)
	_, dy, held := h.Dragged()
	switch {
	case held && st.active != i:
		st.active, st.over, st.lifted, st.keyboard, st.dy = i, i, key, false, 0
		fallthrough
	case held:
		st.dy += dy
		h.Cursor(ui.CursorGrabbing)
		c.Invalidate()
		return false
	case st.active == i && !st.keyboard:
		return true
	}
	if !h.Focused() {
		return false
	}
	lifted := st.active == i && st.keyboard
	switch {
	case h.Shortcut(0, ui.KeySpace) || h.Shortcut(0, ui.KeyEnter):
		if lifted {
			return true
		}
		if st.active < 0 {
			st.active, st.over, st.lifted, st.keyboard, st.dy = i, i, key, true, 0
		}
	case lifted && h.Shortcut(0, ui.KeyDown):
		st.over = min(st.over+1, n-1)
	case lifted && h.Shortcut(0, ui.KeyUp):
		st.over = max(st.over-1, 0)
	case lifted && h.Shortcut(0, ui.KeyEscape):
		st.active = -1
	}
	return false
}

// sortOverlay is the row following the pointer, and settling on a drop.
func sortOverlay[T any](c *ui.Context, list ui.Element, st *sortState, items *[]T, key func(T) string, row func(item T, handle func()), divided bool) {
	idx := st.active
	if idx < 0 && st.dropping != "" {
		for i, it := range *items {
			if key(it) == st.dropping {
				idx = i
			}
		}
	}
	if idx < 0 || idx >= len(st.bounds) || idx >= len(*items) {
		st.dropping = ""
		return
	}
	b := st.bounds[idx]
	var top float32
	switch {
	case st.dropping != "":
		top = list.AnimateTo("overlay", st.dropTop, sortSettle)
		if top == st.dropTop {
			st.dropping = ""
			c.Invalidate()
		}
	case st.keyboard:
		top = list.AnimateTo("overlay", sortSlotTop(st), sortShift)
	default:
		top = list.Jump("overlay", max(0, b.Y+st.dy))
	}
	it := (*items)[idx]
	ui.Overlay(c, func() {
		lifted := ui.Row(c.Key("sortable-overlay")).Absolute().Left(b.X).Top(top).Width(b.W).IgnoreInput(true).AlignItems(ui.Center)
		if divided {
			// A divided row has no look of its own: lifted, it is a
			// floating panel a little wider than its place.
			Floating(c, lifted.Left(b.X-sortLift).Width(b.W+2*sortLift).PaddingX(sortLift))
		}
		lifted.
			Children(func() {
				row(it, func() {
					IconButton(c.Key("sort-handle"), icons.EllipsisVertical, "", 24, false).Radius(12)
				})
			})
	})
}

func sortDrop[T any](c *ui.Context, st *sortState, items *[]T, key func(T) string, moved func(from, to int)) {
	from, to := st.active, st.over
	if from < 0 || from >= len(*items) || to < 0 || to >= len(*items) {
		// A row's own action changed the list in the frame of the drop.
		st.active, st.keyboard, st.dy = -1, false, 0
		return
	}
	if len(st.bounds) > max(from, to) {
		st.dropTop = sortSlotTop(st)
	}
	st.dropping = key((*items)[from])
	st.active, st.keyboard, st.dy = -1, false, 0
	if from != to {
		m := (*items)[from]
		s := append((*items)[:from:from], (*items)[from+1:]...)
		s = append(s[:to], append([]T{m}, s[to:]...)...)
		*items = s
		st.reset = true
		if moved != nil {
			moved(from, to)
		}
	}
	c.Invalidate()
}
