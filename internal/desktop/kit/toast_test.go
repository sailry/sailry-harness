package kit

import (
	"image"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestToastOutcomesExpireWithoutCloseButton(t *testing.T) {
	for _, sample := range []struct {
		name    string
		publish func(*ui.Context, string, func())
		action  bool
	}{
		{"message", func(c *ui.Context, text string, _ func()) { Message(c, text) }, false},
		{"error", func(c *ui.Context, text string, _ func()) { Error(c, text, "Failure detail") }, false},
		{"notification", func(c *ui.Context, text string, _ func()) { Notify(c, text, "Notice detail") }, false},
		{"done", func(c *ui.Context, text string, _ func()) { Toast(c, Done, text) }, false},
		{"failed", func(c *ui.Context, text string, _ func()) { Toast(c, Failed, text) }, false},
		{"conflict", func(c *ui.Context, text string, _ func()) { Toast(c, Conflict, text) }, false},
		{"uncertain", func(c *ui.Context, text string, _ func()) { Toast(c, Uncertain, text) }, false},
		{"retry", Retry, true},
		{"recover failed", func(c *ui.Context, text string, again func()) { Recover(c, Failed, text, again) }, true},
		{"recover uncertain", func(c *ui.Context, text string, again func()) { Recover(c, Uncertain, text, again) }, true},
	} {
		t.Run(sample.name, func(t *testing.T) {
			message := "Toast fixture " + sample.name
			shown, ran := false, 0
			window := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				SetPreferences(c, Preferences{ToastSeconds: 3})
				if !shown {
					shown = true
					sample.publish(c, message, func() { ran++ })
				}
				ToastHost(c)
			}, 600, 400)
			window.SetLocale("en")
			window.SetPreferences(ui.Preferences{ReduceMotion: true, ReduceTransparency: true})
			window.Advance(2 * time.Second)
			if !window.HasText(message) {
				t.Fatalf("the toast ended before its selected duration: %q", window.Texts())
			}
			buttons := 0
			for _, item := range window.AccessAll() {
				if item.Role != ui.RoleButton {
					continue
				}
				buttons++
				if !sample.action || item.Name != i18n.In("en", "chat_retry") {
					t.Fatalf("unexpected toast button: %+v", item)
				}
			}
			if sample.action && buttons != 1 {
				t.Fatalf("the recovery has %d action buttons", buttons)
			}
			window.Advance(1500 * time.Millisecond)
			if window.HasText(message) || ran != 0 {
				t.Fatalf("the toast did not expire silently: actions %d, text %q", ran, window.Texts())
			}
		})
	}
}

func TestToastActionsStillDismiss(t *testing.T) {
	for _, body := range []bool{false, true} {
		name := "button"
		if body {
			name = "body"
		}
		t.Run(name, func(t *testing.T) {
			shown, ran, closed := false, 0, 0
			window := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				if !shown {
					shown = true
					toast := ui.Toast{Title: "Action fixture", Description: "Run the fixture action", Timeout: 3 * time.Second,
						Action: "Apply", OnAction: func() { ran++ }, OnClose: func() { closed++ }}
					if body {
						toast.Data = ToastBodyAction{}
					}
					c.AddToast(toast)
				}
				ToastHost(c)
			}, 600, 400)
			window.SetLocale("en")
			window.Advance(500 * time.Millisecond)
			label := "Apply"
			if body {
				label = "Action fixture Run the fixture action"
			}
			button, ok := window.AccessByRole(ui.RoleButton, label)
			if !ok {
				t.Fatalf("no accessible toast action %q: %+v", label, window.AccessAll())
			}
			for _, item := range window.AccessAll() {
				if item.Role == ui.RoleButton && item.Name != label {
					t.Fatalf("unexpected toast button: %+v", item)
				}
			}
			if body {
				window.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H-4)
			} else {
				window.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
			}
			window.Advance(time.Second)
			if ran != 1 || closed != 1 || window.HasText("Action fixture") {
				t.Fatalf("the action did not close once: actions %d, closures %d, text %q", ran, closed, window.Texts())
			}
		})
	}
}

func TestToastStaysAboveNestedDialogs(t *testing.T) {
	for _, sample := range []struct {
		name   string
		mode   theme.Mode
		reduce bool
	}{{"light frost", theme.Light, false}, {"dark frost", theme.Dark, false},
		{"light opaque", theme.Light, true}, {"dark opaque", theme.Dark, true}} {
		t.Run(sample.name, func(t *testing.T) {
			outer, inner, publish, acted := true, false, true, 0
			var danger ui.Color
			window := ui.NewTesterWith(func(c *ui.Context) {
				theme.Apply(c, "", sample.mode)
				danger = theme.Of(c).Danger
				if publish {
					publish = false
					c.AddToast(ui.Toast{ID: "layer-fixture", Type: "error", Title: "Retry folder read", Timeout: time.Minute,
						Action: "Retry", Data: ToastBodyAction{}, OnAction: func() { acted++ }})
				}
				Dialog(c.Key("outer"), &outer, func() { ui.Box(c).Size(480, 280) })
				Dialog(c.Key("inner"), &inner, func() { ui.Box(c).Size(552, 352) }, DialogOptions{Bleed: true}).Label("Nested folder picker")
				ToastHost(c)
			}, 600, 400, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: sample.reduce}})
			window.SetLocale("en")
			window.Frame()
			window.Advance(time.Second)
			before := window.Image()
			button, ok := window.AccessByRole(ui.RoleButton, "Retry folder read")
			if !ok {
				t.Fatal("the fixture toast did not appear")
			}
			// Sample the error icon's solid pixels, so a backdrop painted
			// over the toast fails even when accessibility and clicks work.
			var ink []image.Point
			for y := int(button.Bounds.Y); y < int(button.Bounds.Y+button.Bounds.H); y++ {
				for x := int(button.Bounds.X); x < int(button.Bounds.X+button.Bounds.W); x++ {
					p := before.RGBAAt(x, y)
					if toastChannelDifference(p.R, danger.R) <= 2 && toastChannelDifference(p.G, danger.G) <= 2 && toastChannelDifference(p.B, danger.B) <= 2 {
						ink = append(ink, image.Pt(x, y))
					}
				}
			}
			if len(ink) < 4 {
				t.Fatalf("the fixture error icon has only %d solid pixels", len(ink))
			}
			// The picker opens after the toast already exists, and its panel
			// covers the toast's screen position. The window still draws the
			// shared toast host last, above every backdrop and dialog panel.
			inner = true
			window.Frame()
			for range 10 {
				window.Advance(40 * time.Millisecond)
				after := window.Image()
				for _, point := range ink {
					p, q := before.RGBAAt(point.X, point.Y), after.RGBAAt(point.X, point.Y)
					if toastChannelDifference(p.R, q.R) > 2 || toastChannelDifference(p.G, q.G) > 2 || toastChannelDifference(p.B, q.B) > 2 {
						t.Fatalf("the nested dialog dimmed or covered the toast at %v: %v became %v", point, p, q)
					}
				}
			}
			button, ok = window.AccessByRole(ui.RoleButton, "Retry folder read")
			if !ok {
				t.Fatalf("the nested dialog hid its toast: %+v", window.AccessAll())
			}
			x, y := button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2
			panel, ok := window.AccessByRole(ui.RoleDialog, "Nested folder picker")
			if !ok || x < panel.Bounds.X || x >= panel.Bounds.X+panel.Bounds.W || y < panel.Bounds.Y || y >= panel.Bounds.Y+panel.Bounds.H {
				t.Fatalf("the fixture toast no longer overlaps the nested dialog: toast=%+v dialog=%+v", button, panel)
			}
			window.ClickAt(x, y)
			window.Frame()
			if acted != 1 || !outer || !inner || window.HasText("Retry folder read") {
				t.Fatalf("toast action hit the dialog: actions=%d outer=%v inner=%v", acted, outer, inner)
			}
		})
	}
}

func toastChannelDifference(a, b uint8) int {
	if a > b {
		return int(a - b)
	}
	return int(b - a)
}

func TestToastPublishedFromOpenDialog(t *testing.T) {
	for _, source := range []string{"dialog action", "queued result"} {
		t.Run(source, func(t *testing.T) {
			outer, inner, queued := true, true, false
			var danger ui.Color
			const message = "Unable to read folder"
			window := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				danger = theme.Of(c).Danger
				if queued {
					queued = false
					Toast(c, Failed, message)
				}
				Dialog(c.Key("outer"), &outer, func() { ui.Box(c).Size(480, 280) })
				Dialog(c.Key("inner"), &inner, func() {
					ui.Column(c).Width(552).Height(352).Padding(40).Children(func() {
						Secondary(c, "Read folder").OnClick(func() {
							if source == "dialog action" {
								Toast(c, Failed, message)
							} else {
								queued = true
							}
						})
					})
				}, DialogOptions{Bleed: true})
				ToastHost(c)
			}, 600, 400)
			window.SetLocale("en")
			window.Frame()
			window.Advance(time.Second)
			if err := window.Click("Read folder"); err != nil {
				t.Fatal(err)
			}
			window.Frame()
			window.Advance(time.Second)
			if !window.HasText(message) || !outer || !inner {
				t.Fatalf("the open modal swallowed the result: outer=%v inner=%v texts=%q", outer, inner, window.Texts())
			}
			var bounds ui.Rect
			for _, item := range window.AccessAll() {
				if item.Role == ui.RoleStatus {
					bounds = item.Bounds
				}
			}
			pixels, solid := window.Image(), 0
			for y := int(bounds.Y); y < int(bounds.Y+bounds.H); y++ {
				for x := int(bounds.X); x < int(bounds.X+bounds.W); x++ {
					p := pixels.RGBAAt(x, y)
					if toastChannelDifference(p.R, danger.R) <= 2 && toastChannelDifference(p.G, danger.G) <= 2 && toastChannelDifference(p.B, danger.B) <= 2 {
						solid++
					}
				}
			}
			if solid < 4 {
				t.Fatalf("the modal covered or dimmed the new toast's error icon: %d solid pixels, bounds=%v", solid, bounds)
			}
		})
	}
}

func TestToastSurfaceResistsModalBackdropDimming(t *testing.T) {
	outer, inner, publish, fresh := false, false, true, false
	window := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		c.Root().Background(theme.Of(c).Background)
		if publish {
			publish = false
			c.AddToast(ui.Toast{ID: "surface-before", Type: "error", Title: "Unable to read folder", Timeout: time.Minute})
		}
		Dialog(c.Key("outer"), &outer, func() { ui.Box(c).Size(640, 440) })
		Dialog(c.Key("inner"), &inner, func() {
			ui.Box(c).Size(952, 680)
			if fresh {
				fresh = false
				c.CloseToast("surface-before")
				c.AddToast(ui.Toast{ID: "surface-after", Type: "error", Title: "Unable to read folder", Timeout: time.Minute})
			}
		}, DialogOptions{Bleed: true}).Label("Folder picker")
		ToastHost(c)
	}, 1000, 1000)
	window.SetLocale("en")
	status := func() ui.Rect {
		t.Helper()
		for _, item := range window.AccessAll() {
			if item.Role == ui.RoleStatus {
				return item.Bounds
			}
		}
		t.Fatal("the fixture toast did not appear")
		return ui.Rect{}
	}
	window.Frame()
	window.Advance(time.Second)
	beforeBounds := status()
	// The lower centre is the surface itself, clear of text, icon, border
	// and rounded corners. A tall window leaves the toast over the modal
	// masks above both centred panels, as in the reported folder picker.
	point := image.Pt(int(beforeBounds.X+beforeBounds.W/2), int(beforeBounds.Y+beforeBounds.H-6))
	before := window.Image().RGBAAt(point.X, point.Y)
	outer, inner = true, true
	window.Frame()
	window.Advance(time.Second)
	fresh = true
	window.Frame()
	window.Advance(time.Second)
	afterBounds := status()
	panel, ok := window.AccessByRole(ui.RoleDialog, "Folder picker")
	if !ok || afterBounds.Y+afterBounds.H >= panel.Bounds.Y || point.X <= int(afterBounds.X) || point.X >= int(afterBounds.X+afterBounds.W) {
		t.Fatalf("the fixture toast no longer sits over the masks outside the panels: toast=%v panel=%+v", afterBounds, panel)
	}
	after := window.Image().RGBAAt(point.X, point.Y)
	if toastChannelDifference(before.R, after.R) > 16 || toastChannelDifference(before.G, after.G) > 16 || toastChannelDifference(before.B, after.B) > 16 {
		t.Fatalf("the modal masks visibly dimmed the foreground notification surface: %v became %v", before, after)
	}
}
