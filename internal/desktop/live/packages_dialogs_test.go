package live

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestPackageConfigurationOnPackagePage asserts a package's settings, opened
// from its own page's header, show there, and that a settings page, which
// draws the dialog itself, does not get a second one from the hook.
func TestPackageConfigurationOnPackagePage(t *testing.T) {
	r := &Runtime{App: &shell.App{Settings: settings.New()}}
	r.App.Settings.Packages = []viewmodel.Package{{ID: "poker", Name: "Poker"}}
	r.App.Settings.OpenPackageConfiguration("poker", "", []viewmodel.Setting{{Name: "player_1_enabled", Title: "Player", Kind: "string"}})
	title := i18n.In("en", "plugins_settings_title")
	draw := func(page shell.Page) bool {
		r.App.Page = page
		tester := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Dark)
			r.packageDialogs(c)
		}, 900, 700)
		tester.SetLocale("en")
		tester.Frame()
		tester.Frame()
		return tester.HasText(title)
	}
	if !draw(shell.PluginPage) {
		t.Fatal("the package's settings do not show on its page")
	}
	if draw(shell.SettingsPage) {
		t.Fatal("the hook drew the dialog over a settings page that draws its own")
	}
}

// TestPackageSettingsTabsAndEfforts asserts a package's tabs reach the
// dialog localized, and an effort setting carries its model's levels.
func TestPackageSettingsTabsAndEfforts(t *testing.T) {
	enabled, model := "p_enabled", "p_model"
	schema := &protocol.PluginSettingsSchema{
		Locales: map[string]map[string]string{"zh-CN": {"tab-1": "玩家 1"}},
		Tabs:    []protocol.PluginSettingsTab{{ID: "tab-1", Title: "Player 1", Fields: []string{enabled, "p_effort"}, Enabled: &enabled}},
		Properties: map[string]protocol.PluginSettingsField{
			enabled:    {Kind: protocol.PluginSettingsKindBoolean},
			model:      {Kind: protocol.PluginSettingsKindString, Model: true},
			"p_effort": {Kind: protocol.PluginSettingsKindString, Effort: &model},
		},
	}
	tabs := packageSettingsTabs(schema, "zh-CN")
	if len(tabs) != 1 || tabs[0].Title != "玩家 1" || tabs[0].Enabled != enabled {
		t.Fatalf("tabs are %+v", tabs)
	}
	catalog := protocol.PluginModelsCatalog{Models: []protocol.PluginModelsModel{{ID: "m1", Efforts: []protocol.Effort{{Kind: "low"}, {Kind: "high"}}}}}
	for _, field := range packageSettingsFields(schema, protocol.PluginSettingsState{}, "en", catalog) {
		if field.Name == "p_effort" && (field.Kind != "effort" || field.Model != model || !slices.Equal(field.Efforts["m1"], []string{"low", "high"})) {
			t.Fatalf("effort field is %+v", field)
		}
	}
}

// TestPackageConfigurationOffSelectedHost asserts a Desktop package's
// settings show while another host, whose list lacks it, is selected.
func TestPackageConfigurationOffSelectedHost(t *testing.T) {
	r := &Runtime{App: &shell.App{Settings: settings.New()}}
	r.App.Page = shell.PluginPage
	r.App.Settings.OpenPackageConfiguration("poker", "Poker", []viewmodel.Setting{{Name: "player_name", Title: "Your name", Kind: "string"}})
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		r.packageDialogs(c)
	}, 900, 700)
	tester.SetLocale("en")
	tester.Frame()
	tester.Frame()
	if !tester.HasText("Your name") || !tester.HasText("Poker · "+i18n.In("en", "plugins_settings_description")) {
		t.Fatalf("the dialog of a package off the selected host is missing: %q", tester.Texts())
	}
}
