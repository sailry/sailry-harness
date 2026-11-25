package shell_test

import (
	"testing"

	"github.com/egoist/mygo/ui"
	harness "github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

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
