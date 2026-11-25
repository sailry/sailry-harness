package shell_test

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	harness "github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestOptionalPanelResourcesInPreview(t *testing.T) {
	for _, test := range []struct {
		name              string
		assembly          shell.Assembly
		browser, terminal bool
	}{
		{name: "empty"},
		{name: "Harness", assembly: harness.Harness(), browser: true},
		{name: "full fixture", assembly: testassembly.Full(), browser: true, terminal: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			app := shell.NewWithAssembly(preview.Load(), test.assembly)
			for _, resource := range []struct {
				kind panel.Kind
				want bool
			}{{panel.Browser, test.browser}, {panel.Terminal, test.terminal}} {
				if app.Panel.Offers(resource.kind) != resource.want {
					t.Fatalf("launcher availability of %q differs from the assembly", resource.kind)
				}
				app.Panel.Open(resource.kind)
				if got := app.Panel.Active; (got.Kind == resource.kind && got.ID != "") != resource.want {
					t.Fatalf("opening %q selected %v", resource.kind, got)
				}
			}
			if app.Panel.BrowserActions != nil || app.Panel.TerminalActions != nil {
				t.Fatal("preview ran the live panel bindings")
			}
		})
	}
}

func TestSampleProjectTerminalMenuFollowsAssembly(t *testing.T) {
	for _, installed := range []bool{false, true} {
		name := "Harness"
		assembly := harness.Harness()
		if installed {
			name = "with Terminal"
			testassembly.RegisterTerminalFixture(&assembly)
		}
		t.Run(name, func(t *testing.T) {
			for _, demo := range []bool{false, true} {
				mode := "preview"
				app := shell.NewWithAssembly(preview.Load(), assembly)
				if demo {
					mode = "demo"
					app = shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)
					app.Settings.Model.DemoMode = true
				}
				t.Run(mode, func(t *testing.T) {
					tester := ui.NewTesterWith(app.View, 1280, 1000, ui.TesterOptions{Preferences: ui.Preferences{
						ReduceTransparency: true, ReduceMotion: true,
					}})
					tester.SetLocale("en")
					tester.SetScale(0.125)
					project := app.Data.Projects[0].Name
					var row ui.Rect
					for _, entry := range tester.AccessAll() {
						if b := entry.Bounds; entry.Name == project && b.X >= shell.RailWidth && b.X < shell.RailWidth+shell.NavWidth && b.Y >= shell.HeaderHeight {
							row = b
							break
						}
					}
					if row.W <= 0 || row.H <= 0 {
						t.Fatalf("no sidebar project row for %q", project)
					}
					tester.RightClickAt(row.X+row.W/2, row.Y+row.H/2)
					label := i18n.In("en", "workspace_new_terminal")
					items := tester.Menu()
					if !slices.Contains(items, i18n.In("en", "workspace_new_session")) || slices.Contains(items, label) != installed {
						t.Fatalf("project menu disagrees with Terminal registration: %q", items)
					}
					if installed {
						if err := tester.ChooseMenuItem(label); err != nil {
							t.Fatal(err)
						}
						tester.Frame()
					}
					if (app.ActiveTerminal() != "") != installed || len(app.Panel.Tabs) != 0 || app.Panel.TerminalActions != nil {
						t.Fatalf("sample project terminal has the wrong scope: main %q, panel %v, live bindings %v",
							app.ActiveTerminal(), app.Panel.Tabs, app.Panel.TerminalActions != nil)
					}
				})
			}
		})
	}
}

func TestOptionalDesktopControls(t *testing.T) {
	for _, test := range []struct {
		name     string
		assembly shell.Assembly
		code     bool
	}{
		{"empty", shell.Assembly{}, false},
		{"Harness", harness.Harness(), false},
		{"Code contributions", testassembly.Full(), true},
	} {
		t.Run(test.name, func(t *testing.T) {
			project := &viewmodel.Project{ID: "fixture", Host: "local", Name: "Fixture", Path: "/fixture"}
			app := shell.NewLiveWithAssembly(&viewmodel.Data{Projects: []*viewmodel.Project{project}, Workspace: &viewmodel.Workspace{}}, test.assembly)
			app.Navigate(shell.SettingsPage)
			app.Settings.Open(settings.Conversation)
			tester := ui.NewTesterWith(app.View, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
			tester.SetLocale("en")
			tester.Frame()
			if tester.HasText(i18n.In("en", "settings_code_index")) != test.code {
				t.Fatal("the built-in code-index control disagrees with the assembly")
			}
			actions := shell.ProjectActions{}
			if app.Settings.HasSection(settings.Terminal) {
				actions.NewTerminal = func(*viewmodel.Project) {}
			}
			app.OpenProject(project, actions)
			tester.Frame()
			label := i18n.In("en", "workspace_new_terminal")
			if _, found := tester.AccessByRole(ui.RoleButton, label); found != test.code {
				t.Fatalf("project terminal button = %v, want %v", found, test.code)
			}
		})
	}
}
