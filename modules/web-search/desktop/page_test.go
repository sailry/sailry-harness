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

func TestSearchDefaultAndRetryActions(t *testing.T) {
	assembly := shell.Assemble(Register)
	catalog := i18n.New(assembly.Locales...)
	en := func(key string) string { return catalog.In("en", key) }
	s := settings.NewLive()
	s.SetNativePages(assembly.SettingsPages)
	s.Open(settings.Search)
	s.Search = settings.SearchSettingsState{Loaded: true, Connected: true, DefaultDriver: "auto", Drivers: []settings.SearchDriver{{ID: "model"}}}
	var actions []string
	s.SearchActions = &settings.SearchActions{
		SetDefault:    func(driver string) { actions = append(actions, driver) },
		SetFallback:   func(bool) { actions = append(actions, "fallback") },
		Retry:         func() bool { actions = append(actions, "retry"); return true },
		ForgetPending: func() { actions = append(actions, "forget") },
		Reload:        func() { actions = append(actions, "reload") },
	}
	view := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, catalog)
		settings.Page(c, s, nil, settings.Look{})
	}, 900, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	view.SetLocale("en")
	view.Frame()
	click := func(role ui.Role, label string) {
		t.Helper()
		x, ok := view.AccessByRole(role, label)
		if !ok || x.Disabled {
			t.Fatalf("no enabled %q control: %q", label, view.Texts())
		}
		view.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
		view.Frame()
		view.Advance(time.Second)
	}
	click(ui.RoleSwitch, en("search_fallback"))
	click(ui.RoleMenuButton, en("search_default_driver"))
	click(ui.RoleMenuItem, en("search_driver_model"))
	if fallback, ok := view.AccessByRole(ui.RoleSwitch, en("search_fallback")); !ok || !fallback.Disabled || s.Search.DefaultDriver != "model" {
		t.Fatalf("choosing the model driver did not disable fallback: %+v", fallback)
	}
	s.Search.Retryable = true
	view.Frame()
	if driver, ok := view.AccessByRole(ui.RoleMenuButton, en("search_default_driver")); !ok || !driver.Disabled {
		t.Fatal("unknown result allows changing the default driver")
	}
	click(ui.RoleButton, en("search_retry"))
	click(ui.RoleButton, en("search_reload"))
	if !slices.Equal(actions, []string{"fallback", "model", "retry", "forget", "reload"}) {
		t.Fatalf("actions = %v", actions)
	}
}
