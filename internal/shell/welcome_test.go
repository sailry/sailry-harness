package shell

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/settings"
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
	if !tt.HasText(i18n.In("en", "onboarding_title")) || !tt.HasText(i18n.In("en", "telemetry_welcome")) {
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
	clickButton(t, tt, i18n.In("en", "message_quote"))
	tt.Frame()
	if a.selected.open || !strings.Contains(a.Composer(a.Sidebar.Selected).Draft, "Preview selection") {
		t.Error("Quote did not reach the draft")
	}
}
