package kit

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The split workspace's dock (gpui-component crates/base/src/dock,
// crates/component/src/dock): panes side by side along an axis with a
// resize handle between each two, the zones a dragged pane drops into,
// and the dragged item's look.
//
// Custom on MyGo bases: ui.Split and SplitPercent share the room between
// two panes only, so a dock split of three to nine children would nest
// splits whose shares no longer mean the children's own; their divider
// paints MyGo's Border-to-Accent line, not Kit's handle; and they keep a
// pane 40 DIPs wide. MyGo has no drop zones within an element either:
// ui.Drop and ui.DragOver take a value over the whole element, so the
// zones are picked here from the pointer's place in it.

// DockMin is the least length a dock pane keeps when its neighbour is
// resized.
const DockMin = 120

// dockGrip is the width of a dock handle's hit area, centred on the 1
// DIP line between two panes; the line drawn while it is held or hovered
// is 2 DIPs, as the shell's column handles.
const dockGrip = 6

// DockSplit lays out len(sizes) panes along an axis, top to bottom when
// vertical, each taking its share of sizes (which sum to 1) of the room,
// keyed by keys so a pane keeps its state as the tree around it changes.
// Between each two is Kit's split handle: a 1 DIP border line at rest,
// the 2 DIP ring line of the shell's handles while the pointer is over
// it or drags it; dragging moves the share between the two panes. It
// reports whether a share changed.
func DockSplit(c *ui.Context, vertical bool, sizes []float32, keys []any, pane func(i int)) bool {
	stack := ui.Row(c)
	if vertical {
		stack = ui.Column(c)
	}
	stack.Grow(1).MinWidth(0).MinHeight(0).AlignItems(ui.Stretch)
	r := stack.Bounds()
	room := r.W
	if vertical {
		room = r.H
	}
	room -= float32(len(sizes) - 1)
	changed := false
	stack.Children(func() {
		for i := range sizes {
			if i > 0 && dockHandle(c, vertical, sizes, i, room) {
				changed = true
			}
			slot := ui.Column(c.Key(keys[i])).Basis(0).Grow(sizes[i]).Shrink(1).MinWidth(0).MinHeight(0).AlignItems(ui.Stretch).Clip()
			slot.Children(func() { pane(i) })
		}
	})
	return changed
}

// dockHandle is the handle between panes i-1 and i, moving the share
// between them by the pointer's travel over room, each keeping DockMin.
func dockHandle(c *ui.Context, vertical bool, sizes []float32, i int, room float32) bool {
	p := theme.Of(c)
	h := ui.Box(c.Key(dockKey{i})).Shrink(0).ZIndex(1).Role(ui.RoleSplitter).Justify(ui.Center).AlignItems(ui.Center)
	line := func(n ui.Element, w float32) ui.Element {
		if vertical {
			return n.Height(w).FillWidth()
		}
		return n.Width(w).FillHeight()
	}
	// The grip overlaps both panes by the same amount, leaving the 1 DIP
	// line in the flow between them.
	if vertical {
		h.Height(dockGrip).Margin(-(dockGrip-1)/2, 0).Cursor(ui.CursorResizeNS)
	} else {
		h.Width(dockGrip).Margin(0, -(dockGrip-1)/2).Cursor(ui.CursorResizeEW)
	}
	dx, dy, held := h.Dragged()
	h.Children(func() {
		if held || h.Hovered() {
			line(ui.Box(c), 2).Background(p.Ring.Alpha(0.5))
			return
		}
		line(ui.Box(c), 1).Background(p.Border)
	})
	d := dx
	if vertical {
		d = dy
	}
	if !held || d == 0 || room <= 0 {
		return false
	}
	pair := sizes[i-1] + sizes[i]
	least := min(DockMin/room, pair/2)
	first := min(max(sizes[i-1]+d/room, least), pair-least)
	if first == sizes[i-1] {
		return false
	}
	sizes[i-1], sizes[i] = first, pair-first
	return true
}

type dockKey struct{ i int }

// Zone is where a dragged pane drops on another: beside it on a side, or over
// it at the centre.
type Zone int

const (
	ZoneCenter Zone = iota
	ZoneLeft
	ZoneRight
	ZoneTop
	ZoneBottom
)

// ZoneAt is the zone of a point within a w by h pane: within 35% of the left
// or right edge a side, then within 35% of the top or bottom, else the
// centre.
func ZoneAt(x, y, w, h float32) Zone {
	switch {
	case x < w*0.35:
		return ZoneLeft
	case x > w*0.65:
		return ZoneRight
	case y < h*0.35:
		return ZoneTop
	case y > h*0.65:
		return ZoneBottom
	}
	return ZoneCenter
}

// zoneRect is the part of a w by h pane a drop at z fills: the half on its
// side, or all of it.
func zoneRect(z Zone, w, h float32) ui.Rect {
	switch z {
	case ZoneLeft:
		return ui.Rect{W: w / 2, H: h}
	case ZoneRight:
		return ui.Rect{X: w / 2, W: w / 2, H: h}
	case ZoneTop:
		return ui.Rect{W: w, H: h / 2}
	case ZoneBottom:
		return ui.Rect{Y: h / 2, W: w, H: h / 2}
	}
	return ui.Rect{W: w, H: h}
}

// dockMotion moves the drop placeholder between zones, as Kit's
// spring_move does.
var dockMotion = ui.Animation{Duration: 180 * time.Millisecond, Ease: ui.CubicBezier(0.2, 0, 0, 1)}

// DockDrop makes e take values of type T dropped on its zones, building its
// children with content. While a T that allow takes at the zone under the
// pointer is dragged over it, the zone's part of e shows in the drop target's
// fill, gliding as the zone changes. It returns the value dropped since the
// last frame and its zone.
func DockDrop[T any](c *ui.Context, e ui.Element, allow func(T, Zone) bool, content func()) (T, Zone, bool) {
	r := e.Bounds()
	zone := func() Zone {
		x, y, _ := e.PointerPosition()
		return ZoneAt(x, y, r.W, r.H)
	}
	var (
		shown bool
		at    ui.Rect
	)
	if v, over := ui.DragOver[T](e); over {
		if z := zone(); allow(v, z) {
			shown, at = true, zoneRect(z, r.W, r.H)
		}
	}
	e.Children(func() {
		content()
		if !shown {
			return
		}
		ind := ui.Box(c.Key(dockKey{-1})).Absolute().PassThrough().Background(theme.Of(c).Primary.Alpha(0.2))
		ind.Left(ind.AnimateTo("x", at.X, dockMotion)).Top(ind.AnimateTo("y", at.Y, dockMotion)).
			Width(ind.AnimateTo("w", at.W, dockMotion)).Height(ind.AnimateTo("h", at.H, dockMotion))
	})
	v, ok := ui.Drop[T](e)
	if !ok {
		return v, ZoneCenter, false
	}
	z := zone()
	if !allow(v, z) {
		return v, z, false
	}
	return v, z, true
}

// DragChip gives e, which drags a pane's target, the look of the dragged item
// while it drags: the title in a popover chip, at Kit's medium radius, 12 by
// 8 DIPs in. MyGo paints a copy of the dragging element under the pointer and
// has no other preview for a drag within the window, so the source wears the
// preview's look. It reports whether e drags, the chip then being its
// content.
func DragChip(c *ui.Context, e ui.Element, title string) bool {
	if !e.Dragging() {
		return false
	}
	p := theme.Of(c)
	e.Background(p.Popover).TextColor(p.PopoverForeground).Radius(p.Radius).Padding(8, 12).Children(func() {
		ui.Text(c, title).FontSize(14).SingleLine().Shrink(1)
	})
	return true
}
