package shell_test

import (
	"maps"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	market "github.com/sailry/sailry-harness/modules/market/desktop"
)

func init() { shell.TestHarnessAssembly = testassembly.Full }

func TestNativePageAssembly(t *testing.T) {
	for _, mode := range []struct {
		name string
		open func(shell.Assembly) *shell.App
		core []string
	}{
		{"preview", func(assembly shell.Assembly) *shell.App { return shell.NewWithAssembly(preview.Load(), assembly) }, []string{string(shell.ConversationPage), string(shell.ActivityPage)}},
		{"live", func(assembly shell.Assembly) *shell.App {
			return shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)
		}, []string{string(shell.ConversationPage)}},
	} {
		t.Run(mode.name, func(t *testing.T) {
			assembly := shell.Assemble(testassembly.RegisterFilesFixture, testassembly.Register, market.Register)
			pages := assembly.WorkspacePages
			full := mode.open(assembly)
			without := mode.open(shell.Assembly{})
			ids := []string{string(shell.FilesPage), string(shell.GitPage), string(shell.MarketPage)}
			want := append(slices.Clone(mode.core), ids...)
			if order, _ := full.FeaturePreferences(); !slices.Equal(order, want) {
				t.Fatalf("assembled rail order = %v, want %v", order, want)
			}
			if order, _ := without.FeaturePreferences(); !slices.Equal(order, mode.core) {
				t.Fatalf("rail order with modules omitted = %v, want %v", order, mode.core)
			}
			catalog := i18n.New(assembly.Locales...)
			menuLabels := map[string]string{}
			for _, group := range full.Menu("en", full.Do) {
				if group.Label != i18n.In("en", "menu_view") {
					continue
				}
				for _, item := range group.Submenu {
					menuLabels[item.ID] = item.Label
				}
			}
			for _, page := range pages {
				key := page.Feature.Label
				if got, label := menuLabels[page.Feature.ID], catalog.In("en", key); got != label || got == key {
					t.Errorf("assembled page %q menu label = %q, want %q", page.Page, got, label)
				}
			}

			full.SetFeaturePreferences(append(slices.Clone(mode.core), ids[1], ids[0], ids[2]), map[string]bool{ids[0]: false, ids[1]: true})
			order, pins := full.FeaturePreferences()
			prefsDir := t.TempDir()
			restore := func(from, to *shell.App) {
				if !from.Preview {
					order, pins := from.FeaturePreferences()
					to.SetFeaturePreferences(order, pins)
					return
				}
				if err := shell.SavePrefs(prefsDir, from.Prefs()); err != nil {
					t.Fatal(err)
				}
				to.SetPrefs(shell.LoadPrefs(prefsDir))
			}
			restore(full, without)
			tester := nativePageTester(without)
			menu := without.Menu("en", without.Do)
			for _, feature := range pages {
				without.Navigate(feature.Page)
				without.Do(feature.Feature.ID)
				tester.Frame()
				if without.Page != shell.ConversationPage {
					t.Fatalf("absent page %q changed navigation to %q", feature.Page, without.Page)
				}
				label := catalog.In("en", feature.Feature.Label)
				if _, found := tester.AccessByRole(ui.RoleButton, label); found {
					t.Errorf("absent page %q has a rail button", feature.Page)
				}
				for _, group := range menu {
					for _, item := range group.Submenu {
						if item.ID == feature.Feature.ID {
							t.Errorf("absent page %q has a native menu item", feature.Page)
						}
					}
				}
			}
			restored := mode.open(assembly)
			restore(without, restored)
			if got, gotPins := restored.FeaturePreferences(); !slices.Equal(got, order) || !maps.Equal(gotPins, pins) {
				t.Fatalf("restored rail order = %v and pins = %v", got, gotPins)
			}
		})
	}
}

func TestThirdPartyNativePage(t *testing.T) {
	const id shell.Page = "example.com/desktop/notes"
	page := shell.NativePage{
		Page:    id,
		Feature: shell.Feature{ID: string(id), Icon: icons.File, Label: "Notes", DefaultPinned: true},
		Render:  func(c *ui.Context, _ *shell.App) { ui.Text(c, "Native notes content") },
	}
	a := shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, page)
	tester := nativePageTester(a)
	a.Do(page.Feature.ID)
	tester.Frame()
	if a.Page != id || !tester.HasText("Native notes content") {
		t.Fatalf("the registered page did not render: page %q, text %q", a.Page, tester.Texts())
	}
	a.ToggleSettings()
	a.ToggleSettings()
	if a.Page != id {
		t.Fatalf("closing Settings returned to %q instead of %q", a.Page, id)
	}
	if !a.ClosePage() || a.Page != shell.ConversationPage {
		t.Fatalf("closing the native page left %q", a.Page)
	}
}

func TestDuplicateNativePages(t *testing.T) {
	first := shell.NativePage{
		Page:    "example.com/desktop/notes",
		Feature: shell.Feature{ID: "example.com/desktop/rail/notes"},
		Render:  func(*ui.Context, *shell.App) {},
	}
	for _, test := range []struct {
		name      string
		page      shell.Page
		featureID string
		want      string
	}{
		{"page ID", first.Page, "example.com/desktop/rail/other", `shell: duplicate native page ID "example.com/desktop/notes"`},
		{"rail ID", "example.com/desktop/other", first.Feature.ID, `shell: duplicate native page rail ID "example.com/desktop/rail/notes"`},
		{"core ID", shell.SettingsPage, "example.com/desktop/rail/other", `shell: duplicate native page ID "page:settings"`},
	} {
		t.Run(test.name, func(t *testing.T) {
			second := first
			second.Page, second.Feature.ID = test.page, test.featureID
			defer func() {
				if got := recover(); got != test.want {
					t.Fatalf("duplicate registration = %v, want %q", got, test.want)
				}
			}()
			shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, first, second)
		})
	}
}

func nativePageTester(app *shell.App) *ui.Tester {
	options := ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}}
	tester := ui.NewTesterWith(app.View, 1280, 820, options)
	tester.SetLocale("en")
	tester.Frame()
	return tester
}
