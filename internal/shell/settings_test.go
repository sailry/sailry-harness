package shell

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/settings"
	"github.com/sailry/sailry-go/internal/theme"
)

// Every section of the navigation opens with its title, in both themes
// and both languages.
func TestSettingsSectionsOpen(t *testing.T) {
	for _, lang := range []string{"en", "zh-CN"} {
		for _, mode := range []theme.Mode{theme.Light, theme.Dark} {
			a, tt := newTester(t)
			tt.SetLocale(lang)
			a.Mode = mode
			tt.Frame()
			click(t, tt, i18n.In(lang, "settings"))
			for _, g := range settings.Groups {
				for _, it := range g.Items {
					title := i18n.In(lang, string(it.Section))
					a.Settings.Open(it.Section)
					tt.Frame()
					if !tt.HasText(title) {
						t.Errorf("%s %v: %s lacks its title %q", lang, mode, it.Section, title)
					}
					if lang == "en" {
						snapshot(t, tt, string(it.Section)+map[theme.Mode]string{theme.Light: "-light", theme.Dark: "-dark"}[mode])
					}
				}
			}
		}
	}
}

// ⌘/ opens Shortcuts; a recorded key replaces the default, the window
// does not act on keys while recording, and refused keys toast.
func TestShortcutRecorder(t *testing.T) {
	a, tt := newTester(t)
	tt.Key(ui.Cmd, ui.KeySlash)
	if a.Page != SettingsPage || a.Settings.Section != settings.Shortcuts {
		t.Fatalf("⌘/ opened %d %q", a.Page, a.Settings.Section)
	}
	click(t, tt, kit.KeyText(ui.Cmd, ui.KeyN))
	if a.Settings.Keys.Active != "conversation.new" {
		t.Fatalf("not recording: %q", a.Settings.Keys.Active)
	}
	// Another shortcut's key is refused, and the window does not search.
	tt.Key(ui.Cmd, ui.KeyK)
	if a.search.open || !tt.HasText(i18n.In("en", "shortcut_conflict")) || !a.Settings.Keys.Recording() {
		t.Fatalf("conflict not refused: search %v recording %v %q", a.search.open, a.Settings.Keys.Recording(), tt.Texts())
	}
	tt.Key(ui.Cmd|ui.Shift, ui.KeyN)
	if a.Settings.Keys.Recording() || a.Page != SettingsPage {
		t.Fatalf("key not taken: recording %v page %d", a.Settings.Keys.Recording(), a.Page)
	}
	if !tt.HasText(kit.KeyText(ui.Cmd|ui.Shift, ui.KeyN)) {
		t.Errorf("new key not shown: %q", tt.Texts())
	}
	// The old key does nothing now; the new one makes a conversation.
	tt.Key(ui.Cmd, ui.KeyN)
	if a.Page != SettingsPage {
		t.Fatalf("old key still bound")
	}
	tt.Key(ui.Cmd|ui.Shift, ui.KeyN)
	if a.Page != ConversationPage {
		t.Fatalf("new key not bound: page %d", a.Page)
	}
	// Escape ends recording, keeping the key.
	tt.Key(ui.Cmd, ui.KeySlash)
	click(t, tt, kit.KeyText(ui.Cmd|ui.Shift, ui.KeyN))
	tt.Key(0, ui.KeyEscape)
	if a.Settings.Keys.Recording() {
		t.Fatalf("Escape did not cancel")
	}
}

// A Node setting's save reports its outcome as a toast.
func TestSettingsSaveToasts(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, i18n.In("en", "settings"))
	sw, ok := tt.AccessByRole(ui.RoleSwitch, i18n.In("en", "power_prevent_sleep"))
	if !ok {
		t.Fatalf("no power switch: %q", tt.Texts())
	}
	tt.ClickAt(sw.Bounds.X+sw.Bounds.W/2, sw.Bounds.Y+sw.Bounds.H/2)
	if !a.Settings.Power.Saved || !tt.HasText(i18n.In("en", "power_settings_saved")) {
		t.Errorf("save not reported: %v %q", a.Settings.Power.Saved, tt.Texts())
	}
}
