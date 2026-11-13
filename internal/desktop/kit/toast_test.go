package kit

import (
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
				window.ClickAt(button.Bounds.X+4, button.Bounds.Y+button.Bounds.H-4)
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
