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

func TestDialogHeaderAndFooterSpacing(t *testing.T) {
	uitest.Parallel(t)

	t.Run("DialogHeader clears close button and keeps 20/24 DIP rhythm", func(t *testing.T) {
		open := true
		tt := uitest.Fast(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			Dialog(c.Key("modal"), &open, func() {
				DialogHeader(c, "A deliberately long dialog title that should stay clear of the close button", "Optional subtitle")
				ui.Box(c.Key("body")).FillWidth().Height(40).Role(ui.RoleRegion).Label("modal-body")
				DialogFooter(c, func() {
					Secondary(c.Key("cancel"), "Cancel")
					Primary(c.Key("save"), "Save", false).Children(func() {
						ui.Text(c, "Save").FontSize(14)
					})
				})
			}).Width(360).Label("modal")
		}, 600, 480, "")
		tt.Frame()
		tt.Advance(dialogIn.Duration)
		tt.Frame()

		closeBtn, ok := tt.AccessByRole(ui.RoleButton, "Close")
		if !ok {
			t.Fatalf("missing Close button: %+v", tt.AccessAll())
		}
		body, ok := tt.AccessByRole(ui.RoleRegion, "modal-body")
		if !ok {
			t.Fatalf("missing modal-body: %+v", tt.AccessAll())
		}
		saveBtn, ok := tt.AccessByRole(ui.RoleButton, "Save")
		if !ok {
			t.Fatalf("missing Save button: %+v", tt.AccessAll())
		}
		var sub ui.AccessInfo
		foundSub := false
		for _, info := range tt.AccessAll() {
			if info.Name == "Optional subtitle" {
				sub = info
				foundSub = true
				break
			}
		}
		if !foundSub {
			t.Fatalf("missing subtitle: %+v", tt.AccessAll())
		}
		// Header content right edge must clear the top-right 28x28 close button.
		if sub.Bounds.X+sub.Bounds.W > closeBtn.Bounds.X {
			t.Fatalf("header overlaps close button: sub=%+v close=%+v", sub.Bounds, closeBtn.Bounds)
		}
		// Header bottom padding (8) + dialogGap (12) = 20 DIP before body.
		headerGap := body.Bounds.Y - (sub.Bounds.Y + sub.Bounds.H)
		if headerGap < 19 || headerGap > 21 {
			t.Fatalf("header-to-body gap = %.1f, want 20 (sub=%+v body=%+v)", headerGap, sub.Bounds, body.Bounds)
		}
		// dialogGap (12) + footer top padding (12) = 24 DIP above footer actions.
		footerGap := saveBtn.Bounds.Y - (body.Bounds.Y + body.Bounds.H)
		if footerGap < 23 || footerGap > 25 {
			t.Fatalf("body-to-footer gap = %.1f, want 24 (body=%+v save=%+v)", footerGap, body.Bounds, saveBtn.Bounds)
		}
	})

	t.Run("DialogHeaderRow matches close-button inset and 20 DIP header gap", func(t *testing.T) {
		open := true
		tt := uitest.Fast(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			Dialog(c.Key("modal-row"), &open, func() {
				DialogHeaderRow(c, func() {
					ui.Box(c.Key("row-fill")).Grow(1).Height(24).Role(ui.RoleRegion).Label("header-row")
				})
				ui.Box(c.Key("body")).FillWidth().Height(40).Role(ui.RoleRegion).Label("modal-body")
				DialogFooter(c, func() {
					Secondary(c.Key("cancel"), "Cancel")
				})
			}).Width(360).Label("modal-row")
		}, 600, 480, "")
		tt.Frame()
		tt.Advance(dialogIn.Duration)
		tt.Frame()

		closeBtn, ok := tt.AccessByRole(ui.RoleButton, "Close")
		if !ok {
			t.Fatalf("missing Close button: %+v", tt.AccessAll())
		}
		head, ok := tt.AccessByRole(ui.RoleRegion, "header-row")
		if !ok {
			t.Fatalf("missing header-row: %+v", tt.AccessAll())
		}
		body, ok := tt.AccessByRole(ui.RoleRegion, "modal-body")
		if !ok {
			t.Fatalf("missing modal-body: %+v", tt.AccessAll())
		}
		cancelBtn, ok := tt.AccessByRole(ui.RoleButton, "Cancel")
		if !ok {
			t.Fatalf("missing Cancel button: %+v", tt.AccessAll())
		}
		// Panel padding (20) + dialogHeaderInset (32) = 52 vs close button 12+28=40 -> 12 DIP clear.
		if clearance := closeBtn.Bounds.X - (head.Bounds.X + head.Bounds.W); clearance < 10 {
			t.Fatalf("header row clearance from close button = %.1f, want >= 10 (head=%+v close=%+v)", clearance, head.Bounds, closeBtn.Bounds)
		}
		if headerGap := body.Bounds.Y - (head.Bounds.Y + head.Bounds.H); headerGap < 19 || headerGap > 21 {
			t.Fatalf("header-row-to-body gap = %.1f, want 20", headerGap)
		}
		if footerGap := cancelBtn.Bounds.Y - (body.Bounds.Y + body.Bounds.H); footerGap < 23 || footerGap > 25 {
			t.Fatalf("body-to-footer gap = %.1f, want 24", footerGap)
		}
	})
}
