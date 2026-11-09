package shell

import (
	"image"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
)

// changed counts the pixels inside r that differ between a and b.
func changed(a, b *image.RGBA, r ui.Rect) int {
	s := float32(a.Bounds().Dx()) / 1280
	n := 0
	for y := int(r.Y * s); y < int((r.Y+r.H)*s); y++ {
		for x := int(r.X * s); x < int((r.X+r.W)*s); x++ {
			p, q := a.RGBAAt(x, y), b.RGBAAt(x, y)
			if d := int(p.R) - int(q.R) + int(p.G) - int(q.G) + int(p.B) - int(q.B); d > 30 || d < -30 {
				n++
			}
		}
	}
	return n
}

// panels returns the boxes of the overlay panels of role showing.
func panels(tt *ui.Tester, role ui.Role) []ui.Rect {
	var out []ui.Rect
	for _, a := range tt.AccessAll() {
		if a.Role == role {
			out = append(out, a.Bounds)
		}
	}
	return out
}

// hoverStill rests the pointer on the tooltip anchor at x, y and moves it
// by a DIP, frame by frame, while the open panel of role stays: no
// tooltip shows (it would draw beneath the panel), the panel's pixels
// never change, so its entrance never replays, and it stays one panel.
func hoverStill(t *testing.T, tt *ui.Tester, role ui.Role, x, y float32) {
	t.Helper()
	ps := panels(tt, role)
	if len(ps) != 1 {
		t.Fatalf("%d panels open, want 1", len(ps))
	}
	panel := ps[0]
	base := tt.Image()
	for i := range 40 {
		tt.Move(x+float32(i%2), y)
		tt.Advance(50 * time.Millisecond)
		tt.Frame()
		if tips := panels(tt, ui.RoleTooltip); len(tips) > 0 {
			t.Fatalf("frame %d: a tooltip shows at %v while a %v is open", i, tips[0], role)
		}
		if n := changed(base, tt.Image(), panel); n > 0 {
			t.Fatalf("frame %d: %d pixels of the open panel changed as the tooltip showed", i, n)
		}
		if got := len(panels(tt, role)); got != 1 {
			t.Fatalf("frame %d: %d panels, want 1", i, got)
		}
	}
}

// restTip rests the pointer at x, y and reports whether a tooltip shows.
func restTip(tt *ui.Tester, x, y float32) bool {
	tt.Move(x, y)
	for range 30 {
		tt.Advance(50 * time.Millisecond)
		tt.Frame()
		if len(panels(tt, ui.RoleTooltip)) > 0 {
			return true
		}
	}
	return false
}

// A tooltip showing beside an open popover leaves it be. Overlay panels
// were named by their place in MyGo's overlay layer, so the context ring's
// tooltip, built before the model chip, took the model popover's place:
// the popover replayed its entrance and the tip drew from its old frames.
func TestTooltipKeepsPopover(t *testing.T) {
	a, tt := newTester(t)
	m := a.Composer(a.Sidebar.Selected).Chosen(a.Data.Models)
	chip, ok := tt.Access(i18n.In("en", "composer_model_settings") + ": " + m.Name)
	if !ok {
		t.Fatal("no model chip")
	}
	tt.ClickAt(chip.Bounds.X+chip.Bounds.W/2, chip.Bounds.Y+chip.Bounds.H/2)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	r := ring(t, a, tt)
	hoverStill(t, tt, ui.RolePopup, r.X+10, r.Y+10)
	snapshot(t, tt, "popover-tooltip")
	// Closed, the ring's tooltip shows again.
	tt.Key(0, ui.KeyEscape)
	tt.Advance(time.Second)
	tt.Frame()
	if len(panels(tt, ui.RolePopup)) != 0 {
		t.Fatal("Escape left the popover open")
	}
	if !restTip(tt, r.X+10, r.Y+10) {
		t.Error("the ring's tooltip does not show once the popover closed")
	}
}

// With nothing open, tooltips show as before.
func TestTooltipShowsWhenNothingOpen(t *testing.T) {
	a, tt := newTester(t)
	r := ring(t, a, tt)
	if !restTip(tt, r.X+10, r.Y+10) {
		t.Fatal("the ring's tooltip does not show")
	}
	attach, ok := tt.Access(i18n.In("en", "composer_attachment"))
	if !ok {
		t.Fatal("no attach button")
	}
	if b := attach.Bounds; !restTip(tt, b.X+b.W/2, b.Y+b.H/2) {
		t.Error("the attach button's tooltip does not show")
	}
}

// The same with a menu open and the attach button's tooltip, built before
// the menu's button.
func TestTooltipKeepsMenu(t *testing.T) {
	_, tt := newTester(t)
	clickItem(t, tt, i18n.In("en", "composer_mode_code"))
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	attach, ok := tt.Access(i18n.In("en", "composer_attachment"))
	if !ok {
		t.Fatal("no attach button")
	}
	b := attach.Bounds
	hoverStill(t, tt, ui.RoleMenu, b.X+b.W/2, b.Y+b.H/2)
}
