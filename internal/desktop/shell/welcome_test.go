package shell

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// A first launch opens the welcome guide; a file from before it counts as
// seen. A provider chosen in it opens the add editor in Settings ›
// Providers, whose Back reopens the guide.
func TestWelcomeGuide(t *testing.T) {
	dir := t.TempDir()
	if p := LoadPrefs(dir); p.WelcomeSeen == nil || *p.WelcomeSeen {
		t.Fatal("a first launch has seen the guide")
	}
	if err := os.WriteFile(filepath.Join(dir, PrefsFile), []byte(`{"navOpen":true}`), 0o644); err != nil {
		t.Fatal(err)
	}
	if p := LoadPrefs(dir); p.WelcomeSeen != nil {
		t.Fatal("an older file asks for the guide")
	}
	a, tt := newTester(t)
	seen := false
	a.SetPrefs(Prefs{NavWidth: NavWidth, NavOpen: true, PanelWidth: PanelWidth, WelcomeSeen: &seen})
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if !tt.HasText(i18n.In("en", "onboarding_title")) {
		t.Fatalf("no guide: %q", tt.Texts())
	}
	for range 3 {
		tt.Advance(time.Second)
		tt.Frame()
	}
	snapshot(t, tt, "welcome-guide")
	clickButton(t, tt, "DeepSeek")
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if a.Welcome.Open || !a.Welcome.Seen || a.Page != SettingsPage || a.Settings.Section != settings.Providers {
		t.Fatalf("choice: %+v page %d %s", a.Welcome, a.Page, a.Settings.Section)
	}
	clickButton(t, tt, i18n.In("en", "provider_step_back"))
	tt.Frame()
	if !a.Welcome.Open {
		t.Error("Back did not return to the guide")
	}
	if p := a.Prefs(); p.WelcomeSeen == nil || !*p.WelcomeSeen {
		t.Error("the guide is not remembered as seen")
	}
}

// The selection shortcut shows the captured text; Quote adds it to the
// conversation's draft. ⌃⌥Space is the activate shortcut's key.
func TestSelection(t *testing.T) {
	a, tt := newTester(t)
	if a.SystemShortcuts()["Ctrl+Alt+Space"] != "system.activate" {
		t.Errorf("system shortcuts %v", a.SystemShortcuts())
	}
	a.Do("system.selection")
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if !tt.HasText(i18n.In("en", "selection_title")) {
		t.Fatalf("no dialog: %q", tt.Texts())
	}
	// SHELL-036: with a reader bound, Read aloud hands it the dialog's text,
	// shows Stop reading while it reads and stops on the next press.
	var read []string
	reading := false
	a.Hooks.ReadSelection = func(chat, text string) { read, reading = append(read, chat+"\x00"+text), !reading }
	a.Hooks.ReaderActive = func(id string) bool { return reading && id == "selection/"+a.selected.chat }
	a.Preview = false
	clickButton(t, tt, i18n.In("en", "reader_read"))
	tt.Frame()
	if _, stop := tt.AccessByRole(ui.RoleButton, i18n.In("en", "reader_stop")); !stop || len(read) != 1 || read[0] != a.Sidebar.Selected+"\x00"+a.selected.text || a.selected.text == "" {
		t.Fatalf("Read aloud read %q; texts %q", read, tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "reader_stop"))
	tt.Frame()
	if _, start := tt.AccessByRole(ui.RoleButton, i18n.In("en", "reader_read")); !start || len(read) != 2 || reading {
		t.Fatalf("the second press read %q, reading %v", read, reading)
	}
	a.Preview = true
	clickButton(t, tt, i18n.In("en", "message_quote"))
	tt.Frame()
	if a.selected.open || !strings.Contains(a.Composer(a.Sidebar.Selected).Draft, "Preview selection") {
		t.Error("Quote did not reach the draft")
	}
	// Every line of a selection is quoted, a blank line after the draft and
	// none after the quote, so a second quote follows the first the same way.
	a.Composer(a.Sidebar.Selected).Draft = ""
	for i, want := range []string{"> one\n> two", "> one\n> two\n\n> one\n> two"} {
		a.selected = selection{open: true, text: "one\ntwo\n", chat: a.Sidebar.Selected}
		tt.Frame()
		clickButton(t, tt, i18n.In("en", "message_quote"))
		tt.Frame()
		if got := a.Composer(a.Sidebar.Selected).Draft; got != want {
			t.Errorf("quote %d: draft %q, want %q", i+1, got, want)
		}
	}
}
