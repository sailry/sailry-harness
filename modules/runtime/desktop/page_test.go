package desktop

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
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

func TestPythonIndexSavesPresetsAndSubmitsCustomURL(t *testing.T) {
	s := settings.NewLive()
	s.SetNativePages(shell.Assemble(Register).SettingsPages)
	s.Open(settings.Runtime)
	s.Indexes = []string{"pypi.org", "Mirror", "custom"}
	s.Python.Revision = 7
	var saves []settings.Python
	s.PackageActions = &settings.PackageActions{SavePython: func() { saves = append(saves, s.Python.Draft) }}
	en := func(key string) string { return testCatalog.In("en", key) }
	view := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		settings.Page(c, s, nil, settings.Look{})
	}, 900, 600, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	view.SetLocale("en")
	view.Frame()
	click := func(role ui.Role, label string) {
		t.Helper()
		x, ok := view.AccessByRole(role, label)
		if !ok || x.Disabled {
			t.Fatalf("no enabled %q: %q", label, view.Texts())
		}
		view.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
		view.Frame()
		view.Advance(time.Second)
	}
	click(ui.RoleMenuButton, en("runtime_python_index"))
	click(ui.RoleMenuItem, "Mirror")
	click(ui.RoleMenuButton, en("runtime_python_index"))
	click(ui.RoleMenuItem, en("runtime_python_custom"))
	if !slices.Equal(saves, []settings.Python{{Index: 1}}) || s.Python.Draft.Index != 2 {
		t.Fatalf("selecting Custom saved before submitting a URL: %v", saves)
	}
	click(ui.RoleTextField, en("runtime_python_custom_url"))
	view.Type("https://mirror.example.test/simple")
	view.Frame()
	if len(saves) != 1 {
		t.Fatal("editing a custom URL triggered an early save")
	}
	view.Key(0, ui.KeyEnter)
	view.Frame()
	if !slices.Equal(saves, []settings.Python{{Index: 1}, {Index: 2, Custom: "https://mirror.example.test/simple"}}) || s.Python.Revision != 7 {
		t.Fatalf("custom index submission = %v, revision %d", saves, s.Python.Revision)
	}
}
