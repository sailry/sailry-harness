package shell

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
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
			var items []settings.Item
			for _, group := range settings.Groups {
				items = append(items, group.Items...)
			}
			for _, page := range TestHarnessAssembly().SettingsPages {
				items = append(items, settings.Item{Section: page.Section})
			}
			for _, it := range items {
				title := i18n.In(lang, string(it.Section))
				a.Settings.Open(it.Section)
				tt.Frame()
				if !tt.HasText(title) {
					t.Errorf("%s %v: %s lacks its title %q", lang, mode, it.Section, title)
				}
				name := string(it.Section) + map[theme.Mode]string{theme.Light: "-light", theme.Dark: "-dark"}[mode]
				if lang == "en" {
					snapshot(t, tt, name)
				}
				// Media models is one section of its own: four rows that
				// follow the conversation model, and nothing of it on
				// Models.
				if it.Section != settings.Media && it.Section != settings.Providers {
					continue
				}
				if lang != "en" {
					snapshot(t, tt, name+"-"+lang)
				}
				entries, follows := 0, 0
				for _, x := range tt.AccessAll() {
					if x.Role == ui.RoleButton && x.Name == i18n.In(lang, "settings_media") {
						entries++
					}
					for _, row := range []string{"provider_cap_vision", "media_image", "media_video", "media_speech"} {
						if x.Role == ui.RoleButton && x.HasPopup == ui.PopupListBox && x.Name == i18n.In(lang, row) {
							follows++
						}
					}
				}
				want := map[settings.Section]int{settings.Media: 4, settings.Providers: 0}[it.Section]
				if entries != 1 || follows != want {
					t.Errorf("%s %v: %s has %d Media models entries and %d media selects, want 1 and %d", lang, mode, it.Section, entries, follows, want)
				}
				if it.Section == settings.Media && !tt.HasText(i18n.In(lang, "settings_media_description")) {
					t.Errorf("%s %v: Media models lacks its description", lang, mode)
				}
			}
		}
	}
}

// A key the native menu takes while recording is recorded and returns the
// focus to the shortcut's button, as a key the page takes does; a second
// command sent in the same frame finds no recording and is not refused.
func TestShortcutRecorderMenuKeyRefocuses(t *testing.T) {
	a, tt := newTester(t)
	tt.Key(ui.Cmd, ui.KeySlash)
	click(t, tt, kit.KeyText(ui.Cmd, ui.KeyN))
	name := i18n.In("en", settings.Registry[0].Label)
	if _, focused := focusOf(tt); !a.Settings.Keys.Recording() || focused == name {
		t.Fatalf("recording %v with the focus on %q", a.Settings.Keys.Recording(), focused)
	}
	a.Do(settings.Registry[0].ID)
	a.Do("app.search")
	tt.Frame()
	tt.Frame()
	role, focused := focusOf(tt)
	if a.Settings.Keys.Recording() || a.Page != SettingsPage || a.search.open || role != ui.RoleButton || focused != name {
		t.Fatalf("menu key: recording %v, page %q, search %v, focus %v %q", a.Settings.Keys.Recording(), a.Page, a.search.open, role, focused)
	}
	if tt.HasText(i18n.In("en", "shortcut_unavailable")) || tt.HasText(i18n.In("en", "shortcut_conflict")) {
		t.Errorf("a menu key after the recording ended was refused: %q", tt.Texts())
	}
}

// ⌘/ opens Shortcuts; a recorded key replaces the default, the window
// does not act on keys while recording, and refused keys toast.
func TestShortcutRecorder(t *testing.T) {
	a, tt := newTester(t)
	tt.Key(ui.Cmd, ui.KeySlash)
	if a.Page != SettingsPage || a.Settings.Section != settings.Shortcuts {
		t.Fatalf("⌘/ opened %q %q", a.Page, a.Settings.Section)
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
	// Expiring the conflict's toast leaves the keys with the recorder.
	tt.Advance(500 * time.Millisecond)
	if _, found := tt.AccessByRole(ui.RoleButton, i18n.In("en", "close")); found {
		t.Fatal("the conflict toast has a close button")
	}
	tt.Move(1, 1)
	tt.Advance(time.Duration(settings.ToastSeconds[a.Settings.Toast])*time.Second + time.Second)
	if tt.HasText(i18n.In("en", "shortcut_conflict")) || !a.Settings.Keys.Recording() {
		t.Fatalf("the toast did not expire with the recorder active: %q", tt.Texts())
	}
	tt.Key(ui.Cmd|ui.Shift, ui.KeyN)
	if a.Settings.Keys.Recording() || a.Page != SettingsPage {
		t.Fatalf("key not taken: recording %v page %q", a.Settings.Keys.Recording(), a.Page)
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
		t.Fatalf("new key not bound: page %q", a.Page)
	}
	// Escape ends recording, keeping the key.
	tt.Key(ui.Cmd, ui.KeySlash)
	click(t, tt, kit.KeyText(ui.Cmd|ui.Shift, ui.KeyN))
	tt.Key(0, ui.KeyEscape)
	if a.Settings.Keys.Recording() {
		t.Fatalf("Escape did not cancel")
	}
	// SNAV-002: opening another section ends the recording, from the
	// navigation or by a caller that sets the section itself.
	for _, leave := range []struct {
		name string
		do   func()
	}{
		{"navigation", func() { clickButton(t, tt, i18n.In("en", string(settings.General))) }},
		{"assignment", func() { a.Settings.Section = settings.General }},
	} {
		click(t, tt, kit.KeyText(ui.Cmd|ui.Shift, ui.KeyN))
		if !a.Settings.Keys.Recording() {
			t.Fatalf("%s: not recording", leave.name)
		}
		leave.do()
		tt.Frame()
		if a.Settings.Section != settings.General || a.Settings.Keys.Recording() {
			t.Fatalf("%s: section %q, recording %v", leave.name, a.Settings.Section, a.Settings.Keys.Recording())
		}
		a.Settings.Open(settings.Shortcuts)
		tt.Frame()
	}
	// SNAV-002: leaving the page by the rail ends the recording, so the
	// window's keys and the menu's commands work again and the key is kept.
	click(t, tt, kit.KeyText(ui.Cmd|ui.Shift, ui.KeyN))
	clickButton(t, tt, i18n.In("en", "activity"))
	tt.Frame()
	if a.Page == SettingsPage || a.Settings.Keys.Recording() {
		t.Fatalf("left on page %q, recording %v", a.Page, a.Settings.Keys.Recording())
	}
	// Activity has no side column, so the window's key shown working is the
	// panel's; ⌘B leaves the sidebar's saved choice there.
	open, panel := a.NavOpen, a.PanelOpen
	tt.Key(ui.Cmd, ui.KeyB)
	tt.Key(ui.Cmd|ui.Alt, ui.KeyB)
	a.Do("page:files")
	tt.Frame()
	if a.PanelOpen == panel || a.NavOpen != open || a.Page != FilesPage {
		t.Errorf("after leaving a recording: panel toggled %v, sidebar toggled %v, page %q", a.PanelOpen != panel, a.NavOpen != open, a.Page)
	}
	if s, ok := a.Settings.Keys.Bound(settings.Registry[0]); !ok || s.Mods != ui.Cmd|ui.Shift || s.Key != ui.KeyN {
		t.Errorf("the abandoned recording changed the key: %+v %v", s, ok)
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
	if power := a.Settings.Power; power.Saved || power.Draft || power.Revision != 1 {
		t.Fatalf("the setting starts as %+v", power)
	}
	tt.ClickAt(sw.Bounds.X+sw.Bounds.W/2, sw.Bounds.Y+sw.Bounds.H/2)
	// The preview's save commits the draft as the next revision and is Done.
	if power := a.Settings.Power; !power.Saved || !power.Draft || power.Revision != 2 || !tt.HasText(i18n.In("en", "power_settings_saved")) {
		t.Errorf("save not reported: %+v %q", power, tt.Texts())
	}
}

// The terminal's variable dialog refuses an invalid name with a toast
// and keeps its draft.
func TestTerminalVariableDialog(t *testing.T) {
	a, tt := newTester(t)
	variable := a.translations.In("en", "terminal_variable")
	invalid := a.translations.In("en", "terminal_variable_invalid")
	add := a.translations.In("en", "terminal_environment_add")
	click(t, tt, i18n.In("en", "settings"))
	a.Settings.Open(settings.Terminal)
	tt.Frame()
	click(t, tt, i18n.In("en", "settings_add"))
	click(t, tt, variable)
	tt.Type("1BAD")
	click(t, tt, i18n.In("en", "settings_save"))
	if !tt.HasText(invalid) || !tt.HasText(add) {
		t.Fatalf("invalid name not refused: %q", tt.Texts())
	}
	// The failure toast expires over the open dialog, which keeps its draft.
	tt.Advance(500 * time.Millisecond) // the toast slid in
	message, _ := tt.Find(invalid)
	for _, button := range tt.AccessAll() {
		if button.Role == ui.RoleButton && button.Name == i18n.In("en", "close") &&
			button.Bounds.X > message.X && button.Bounds.X < message.X+320 && // inside the 360 DIP toast, not the dialog's own
			button.Bounds.Y+button.Bounds.H > message.Y && button.Bounds.Y < message.Y+message.H {
			t.Fatal("the failure toast has a close button")
		}
	}
	tt.Move(1, 1)
	tt.Advance(time.Duration(settings.ToastSeconds[a.Settings.Toast])*time.Second + time.Second)
	if tt.HasText(invalid) || !tt.HasText(add) {
		t.Fatalf("failure toast outlived its duration: %q", tt.Texts())
	}
	if field, found := tt.AccessByRole(ui.RoleTextField, variable); !found || field.Value != "1BAD" {
		t.Fatalf("expiring the toast changed the dialog draft: %+v, found %v", field, found)
	}
	n := len(a.Settings.Host.Saved.Env)
	click(t, tt, variable)
	tt.Key(ui.Cmd, ui.KeyA)
	tt.Type("GOOD")
	click(t, tt, i18n.In("en", "settings_save"))
	if len(a.Settings.Host.Saved.Env) != n+1 || tt.HasText(add) {
		t.Fatalf("variable not saved: %+v", a.Settings.Host.Saved.Env)
	}
}

// The plugins header shows Check updates, Download and Install; Update all
// and Review updates follow a check that found updates.
func TestPluginsHeaderActions(t *testing.T) {
	a, tt := newTester(t)
	a.Settings.Open(settings.Plugins)
	a.Page = SettingsPage
	tt.Frame()
	all, review := i18n.In("en", "plugins_update_all"), i18n.In("en", "plugins_review_updates")
	if !tt.HasText(i18n.In("en", "plugins_download")) || tt.HasText(all) || tt.HasText(review) {
		t.Fatalf("header before a check: %q", tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "plugins_check_updates"))
	tt.Frame()
	if !tt.HasText(all) {
		t.Errorf("no Update all after the check: %q", tt.Texts())
	}
}
