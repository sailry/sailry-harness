package kit

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/uitest"
)

func TestDialogKeepsPressWhenEarlierOverlayAppears(t *testing.T) {
	uitest.Parallel(t)
	open, hover := true, false
	clicked := 0
	tt := uitest.Fast(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		background := ui.Box(c.Key("background")).Fill()
		x, _, inside := background.PointerPosition()
		hover = inside && x > 100
		if hover {
			// A chart behind a modal can show a pointer-following tip.
			ui.Overlay(c, func() {
				ui.Box(c.Key("tip")).Absolute().Size(8, 8).IgnoreInput(true)
			})
		}
		Dialog(c.Key("prompt"), &open, func() {
			Primary(c.Key("continue"), "Continue", false).Children(func() {
				ui.Text(c, "Continue").FontSize(14)
			}).OnClick(func() { clicked++ })
		}).Width(240)
	}, 600, 400, "")
	tt.Move(4, 4)
	tt.Frame()
	if hover {
		t.Fatal("the earlier overlay is already present before pressing")
	}
	button, ok := tt.AccessByRole(ui.RoleButton, "Continue")
	if !ok {
		t.Fatal("the dialog action is unavailable")
	}
	x, y := button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2
	tt.Press(x, y)
	if !hover {
		t.Fatal("pressing did not introduce the earlier overlay")
	}
	tt.Release(x, y)
	tt.Frame()
	if clicked != 1 || !open {
		t.Fatalf("dialog action after the overlay appeared: clicked=%d open=%v", clicked, open)
	}
}
