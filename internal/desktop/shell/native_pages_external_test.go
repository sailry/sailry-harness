package shell_test

import (
	"maps"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	desktop "github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	git "github.com/sailry/sailry-harness/modules/git/desktop"
)

func init() { shell.TestHarnessPages = desktop.Harness }

func TestNativePageAssembly(t *testing.T) {
	for _, mode := range []struct {
		name string
		open func(...shell.NativePage) *shell.App
		core []string
	}{
		{"preview", func(pages ...shell.NativePage) *shell.App { return shell.New(preview.Load(), pages...) }, []string{string(shell.ConversationPage), string(shell.ActivityPage)}},
		{"live", func(pages ...shell.NativePage) *shell.App {
			return shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, pages...)
		}, []string{string(shell.ConversationPage)}},
	} {
		t.Run(mode.name, func(t *testing.T) {
			full := mode.open(desktop.Harness()...)
			without := mode.open()
			ids := []string{string(shell.FilesPage), string(shell.GitPage), string(shell.MarketPage)}
			want := append(slices.Clone(mode.core), ids...)
			if order, _ := full.FeaturePreferences(); !slices.Equal(order, want) {
				t.Fatalf("assembled rail order = %v, want %v", order, want)
			}
			if order, _ := without.FeaturePreferences(); !slices.Equal(order, mode.core) {
				t.Fatalf("rail order with modules omitted = %v, want %v", order, mode.core)
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
			for _, feature := range desktop.Harness() {
				without.Navigate(feature.Page)
				without.Do(feature.Feature.ID)
				tester.Frame()
				if without.Page != shell.ConversationPage {
					t.Fatalf("absent page %q changed navigation to %q", feature.Page, without.Page)
				}
				label := i18n.In("en", feature.Feature.Label)
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
			restored := mode.open(desktop.Harness()...)
			restore(without, restored)
			if got, gotPins := restored.FeaturePreferences(); !slices.Equal(got, order) || !maps.Equal(gotPins, pins) {
				t.Fatalf("restored rail order = %v and pins = %v", got, gotPins)
			}
		})
	}
}

func TestGitOpenSource(t *testing.T) {
	for _, test := range []struct {
		name      string
		pages     []shell.NativePage
		canReveal bool
	}{
		{"standalone", []shell.NativePage{git.Page()}, false},
		{"with Files", desktop.Harness(), true},
	} {
		t.Run(test.name, func(t *testing.T) {
			a := shell.New(preview.Load(), test.pages...)
			a.Navigate(shell.GitPage)
			tester := nativePageTester(a)
			tester.Advance(time.Second)
			tester.Frame()
			row, ok := tester.AccessByRole(ui.RoleTreeItem, "example.md")
			if a.Page != shell.GitPage || !ok {
				t.Fatalf("the Git page did not render the change: %q", tester.Texts())
			}
			tester.RightClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
			open := i18n.In("en", "git_open_source")
			if menu := tester.Menu(); slices.Contains(menu, open) != test.canReveal || !slices.Contains(menu, i18n.In("en", "git_discard_changes")) {
				t.Fatalf("unexpected change menu: %v", menu)
			}
			if !test.canReveal {
				return
			}
			if err := tester.ChooseMenuItem(open); err != nil {
				t.Fatal(err)
			}
			tester.Frame()
			project := a.ResourceProject()
			if a.Page != shell.FilesPage || project == nil || a.Projects.Of(project.ID).Files.File() != "sailry-go/docs/example.md" {
				t.Fatalf("the source file did not open in Files: %q", tester.Texts())
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
