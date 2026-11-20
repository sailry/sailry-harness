package settings

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// A report's items are grouped by the area each first appears in.
func TestConfigurationGroups(t *testing.T) {
	areas, grouped := ConfigurationGroups([]ConfigurationItem{
		{Area: "providers", Name: "OpenAI"}, {Area: "power"}, {Area: "providers", Name: "Anthropic"},
	})
	if !slices.Equal(areas, []string{"providers", "power"}) || len(grouped["providers"]) != 2 || grouped["providers"][1].Name != "Anthropic" || len(grouped["power"]) != 1 {
		t.Fatalf("grouped %q into %+v", areas, grouped)
	}
}

// Import previews the picked file, lists what it does by area, and imports
// the same document on confirmation. An unconfirmed import keeps the
// dialog open with Retry; closing it gives the kept request up.
func TestConfigurationImportDialog(t *testing.T) {
	m := NewModel(preview.SettingsData())
	tt := ui.NewTesterWith(func(c *ui.Context) {
		ui.Scroll(c).Fill().Children(func() { backup(c, m) })
	}, 1000, 1600, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	en := func(key string) string { return i18n.In("en", key) }
	for _, want := range []string{en("configuration_export"), en("configuration_export_description"), en("configuration_import")} {
		if !tt.HasText(want) {
			t.Fatalf("the Data page does not show %q: %q", want, tt.Texts())
		}
	}
	// The preview stops at the notice.
	clickButton(t, tt, en("configuration_import_action"))
	if m.configuration.open {
		t.Fatal("the preview opened an import")
	}

	document := []byte(`{"version":1}`)
	type call struct {
		document string
		preview  bool
	}
	var calls []call
	var answer func([]ConfigurationItem, bool, bool)
	forgotten := 0
	reason := "plugin is not installed on this Node"
	m.ConfigurationActions = &ConfigurationActions{
		Export: func() {},
		Pick:   func(selected func([]byte)) { selected(document) },
		Import: func(document []byte, preview bool, done func([]ConfigurationItem, bool, bool)) {
			calls, answer = append(calls, call{string(document), preview}), done
		},
		Forget: func() { forgotten++ },
	}
	tt.Frame()
	row := findButton(t, tt, en("configuration_import_action"))
	clickButton(t, tt, en("configuration_import_action"))
	if len(calls) != 1 || !calls[0].preview || calls[0].document != string(document) {
		t.Fatalf("picking a file sent %+v", calls)
	}
	answer([]ConfigurationItem{
		{Area: "providers", Name: "OpenAI", Action: "added"},
		{Area: "providers", Name: "Anthropic", Action: "updated"},
		{Area: "power", Action: "updated"},
		{Area: "plugins", Name: "git", Action: "skipped", Reason: reason},
	}, true, false)
	tt.Frame()
	for _, want := range []string{en("configuration_preview"), en("settings_providers"), "OpenAI", "Anthropic", en("power_prevent_sleep"),
		en("settings_plugins"), "git", reason, en("configuration_added"), en("configuration_updated"), en("configuration_skipped")} {
		if !tt.HasText(want) {
			t.Errorf("the preview does not show %q: %q", want, tt.Texts())
		}
	}
	confirm := func(label string) {
		t.Helper()
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleButton && x.Name == label && x.Bounds != row.Bounds {
				tt.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
				tt.Frame()
				return
			}
		}
		t.Fatalf("no dialog %q button: %q", label, tt.Texts())
	}
	confirm(en("configuration_import_action"))
	if len(calls) != 2 || calls[1] != (call{string(document), false}) {
		t.Fatalf("confirming sent %+v", calls)
	}
	// Unconfirmed: the dialog stays with Retry, which sends the import again.
	answer(nil, false, true)
	tt.Frame()
	if !m.configuration.open {
		t.Fatal("an unconfirmed import closed its dialog")
	}
	confirm(en("plugins_retry"))
	if len(calls) != 3 || calls[2] != (call{string(document), false}) {
		t.Fatalf("Retry sent %+v", calls)
	}
	answer(nil, false, true)
	tt.Frame()
	clickButton(t, tt, en("settings_cancel"))
	tt.Frame()
	if m.configuration.open || forgotten != 1 {
		t.Fatalf("closing an unconfirmed import left it open %v and gave it up %d times", m.configuration.open, forgotten)
	}
	// A confirmed import closes the dialog.
	clickButton(t, tt, en("configuration_import_action"))
	answer(nil, true, false)
	tt.Frame()
	confirm(en("configuration_import_action"))
	answer(nil, true, false)
	tt.Frame()
	tt.Frame()
	if m.configuration.open || forgotten != 1 || len(calls) != 5 || calls[4].preview {
		t.Fatalf("a confirmed import left the dialog open %v after %+v", m.configuration.open, calls)
	}
}
