package desktop

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

var testCatalog = i18n.New(Locales)

func TestPythonPreparations(t *testing.T) {
	state := settings.New()
	state.SetNativePages([]settings.NativePage{Page()})
	state.Open(settings.Runtime)
	state.Live = true
	state.ProviderActions.Hosts = func() []settings.ProviderHost {
		return []settings.ProviderHost{{Node: "local", Local: true, Connected: true}, {Node: "remote", Name: "Remote", Connected: true}}
	}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		settings.Page(c, state, nil, settings.Look{})
	}, 900, 600, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	if _, ok := tt.AccessByRole(ui.RoleMenuButton, testCatalog.In("en", "project_host")); !ok {
		t.Fatalf("no target Node picker: %q", tt.Texts())
	}
	title := testCatalog.In("en", "runtime_python_preparing")
	if tt.HasText(title) {
		t.Fatalf("an idle Node shows the row: %q", tt.Texts())
	}
	state.PythonPreparing = []settings.PythonPreparation{{Package: "charts", Stage: "installing"}, {Package: "ocr", Stage: "validating"}}
	tt.Frame()
	for _, want := range []string{title, "charts", testCatalog.In("en", "python_installing"), "ocr", testCatalog.In("en", "python_validating")} {
		if !tt.HasText(want) {
			t.Fatalf("no %q: %q", want, tt.Texts())
		}
	}
}
