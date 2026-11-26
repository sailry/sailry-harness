package live

import (
	"context"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	desktop "github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// welcomeMarkWindow is a Desktop window on the profile's preferences at path,
// showing a new session's welcome.
func welcomeMarkWindow(t *testing.T, path string) (*Runtime, *ui.Tester) {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	t.Cleanup(cancel)
	snapshot := selectionSnapshot()
	source := &selectionTestTransport{
		conversationTestTransport: conversationTestTransport{snapshot: snapshot},
		node:                      &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})},
	}
	source.node.updates <- protocol.Update{Kind: "snapshot", Data: snapshot}
	controller := client.New(source)
	t.Cleanup(func() { _ = controller.Close() })
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	prefs := preferences.Open(path)
	prefs.Data.WelcomeSeen = new(true)
	r, err := NewWithAssembly(ctx, controller, prefs, desktop.Full())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { r.workers.Wait(); _ = r.Close() })
	window := ui.NewTesterWith(r.App.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	window.SetLocale("en")
	r.newDraft(nil)
	window.Frame()
	window.Frame()
	return r, window
}

// lettering is the welcome's lettering labeled label, which must show.
func lettering(t *testing.T, r *Runtime, window *ui.Tester, label string) {
	t.Helper()
	if r.App.Page != shell.ConversationPage || !window.HasText(i18n.In("en", "welcome_title")) {
		t.Fatalf("no welcome on page %v: %q", r.App.Page, window.Texts())
	}
	if mark, ok := window.AccessByRole(ui.RoleImage, label); !ok || mark.Bounds.W <= 0 || mark.Bounds.H <= 0 {
		t.Fatalf("the welcome shows no lettering %q", label)
	}
}

// storedMark is the lettering the profile's file holds once the window's
// writes are done.
func storedMark(r *Runtime, path string) *string {
	r.workers.Wait()
	return preferences.Open(path).Data.WelcomeMark
}

// click presses the control with role and label.
func click(t *testing.T, window *ui.Tester, role ui.Role, label string) ui.AccessInfo {
	t.Helper()
	field, ok := window.AccessByRole(role, label)
	if !ok || field.Disabled {
		t.Fatalf("no %q to click in %q", label, window.Texts())
	}
	window.ClickAt(field.Bounds.X+field.Bounds.W/2, field.Bounds.Y+field.Bounds.H/2)
	window.Frame()
	return field
}

// focusMarkField scrolls Settings › Appearance to its last group, as a user
// does, and clicks the lettering's field there.
func focusMarkField(t *testing.T, window *ui.Tester, title string) {
	t.Helper()
	for range 20 {
		field, ok := window.AccessByRole(ui.RoleTextField, title)
		if !ok {
			t.Fatalf("no %q field in %q", title, window.Texts())
		}
		if field.Bounds.Y >= 0 && field.Bounds.Y+field.Bounds.H <= 820 {
			break
		}
		window.Wheel(800, 400, 0, 400)
		window.Advance(time.Second)
		window.Frame()
	}
	click(t, window, ui.RoleTextField, title)
	if field, _ := window.AccessByRole(ui.RoleTextField, title); !field.Focused {
		t.Fatalf("the %q field did not take focus at %+v", title, field.Bounds)
	}
}

// openAppearance goes from the welcome to Settings › Appearance by the rail's
// Settings button, the welcome's way to edit its lettering.
func openAppearance(t *testing.T, r *Runtime, window *ui.Tester) {
	t.Helper()
	click(t, window, ui.RoleButton, i18n.In("en", "settings"))
	if err := window.Click(i18n.In("en", "settings_appearance")); err != nil {
		t.Fatal(err)
	}
	window.Frame()
	if r.App.Page != shell.SettingsPage {
		t.Fatalf("Settings did not open: page %v", r.App.Page)
	}
}

// The lettering typed in Settings › Appearance shows on the welcome, is saved
// with its last character while the field keeps focus, survives reopening
// the profile, and clearing it brings back the default lettering.
func TestWelcomeMarkEndToEnd(t *testing.T) {
	path := filepath.Join(t.TempDir(), "desktop", "preferences.json")
	sailry := conversation.DefaultWordmark
	title := i18n.In("en", "appearance_welcome_mark")

	r, window := welcomeMarkWindow(t, path)
	lettering(t, r, window, sailry)
	openAppearance(t, r, window)
	focusMarkField(t, window, title)
	window.Type("Ship it!")
	window.Frame()
	if field, ok := window.AccessByRole(ui.RoleTextField, title); !ok || !field.Focused {
		t.Fatal("the field lost focus while typing")
	}
	if got := storedMark(r, path); got == nil || *got != "Ship it!" {
		t.Fatalf("the last character was not saved with the field focused: %v", got)
	}
	// Characters the lettering cannot draw are not kept.
	window.Type("\u00e9")
	window.Frame()
	if got := storedMark(r, path); got == nil || *got != "Ship it!" || r.App.Messages.Mark != "Ship it!" {
		t.Fatalf("a non-ASCII character was kept: %v, %q", got, r.App.Messages.Mark)
	}
	click(t, window, ui.RoleButton, i18n.In("en", "settings"))
	lettering(t, r, window, "Ship it!")
	r.workers.Wait()
	_ = r.Close()

	r, window = welcomeMarkWindow(t, path)
	lettering(t, r, window, "Ship it!")
	openAppearance(t, r, window)
	focusMarkField(t, window, title)
	for range len("Ship it!") {
		window.Key(0, ui.KeyBackspace)
	}
	window.Frame()
	if got := storedMark(r, path); got != nil {
		t.Fatalf("a cleared lettering was saved as %q", *got)
	}
	click(t, window, ui.RoleButton, i18n.In("en", "settings"))
	lettering(t, r, window, sailry)
	r.workers.Wait()
	_ = r.Close()

	r, window = welcomeMarkWindow(t, path)
	lettering(t, r, window, sailry)
}

func TestWelcomeDefaultMarkEndToEnd(t *testing.T) {
	path := filepath.Join(t.TempDir(), "desktop", "preferences.json")
	title := i18n.In("en", "appearance_welcome_mark")

	r, window := welcomeMarkWindow(t, path)
	r.App.Messages.DefaultMark = "SAILRY HARNESS"
	window.Frame()
	lettering(t, r, window, "SAILRY HARNESS")
	if got := storedMark(r, path); got != nil {
		t.Fatalf("default mark was unexpectedly persisted as %q", *got)
	}

	openAppearance(t, r, window)
	focusMarkField(t, window, title)
	window.Type("MY TEAM")
	window.Frame()
	if got := storedMark(r, path); got == nil || *got != "MY TEAM" {
		t.Fatalf("custom mark not saved: %v", got)
	}
	click(t, window, ui.RoleButton, i18n.In("en", "settings"))
	lettering(t, r, window, "MY TEAM")

	openAppearance(t, r, window)
	focusMarkField(t, window, title)
	for range len("MY TEAM") {
		window.Key(0, ui.KeyBackspace)
	}
	window.Frame()
	if got := storedMark(r, path); got != nil {
		t.Fatalf("cleared mark persisted as %q", *got)
	}
	click(t, window, ui.RoleButton, i18n.In("en", "settings"))
	lettering(t, r, window, "SAILRY HARNESS")
}
