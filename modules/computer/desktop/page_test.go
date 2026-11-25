package desktop

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func TestComputerSettingsKeepsSystemPermissionActionsLocal(t *testing.T) {
	assembly := shell.Assemble(Register)
	catalog := i18n.New(assembly.Locales...)
	en := func(key string) string { return catalog.In("en", key) }
	s := settings.NewLive()
	s.SetNativePages(assembly.SettingsPages)
	s.Open(settings.Computer)
	s.Computer = settings.ComputerSettingsState{
		Loaded: true, HostReadOnly: true,
		Enabled:     settings.Stored[bool]{Saved: true, Draft: true, Revision: 7},
		Permissions: []preview.Permission{{Resource: "permission_accessibility", Status: "permission_denied"}},
	}
	var requests []string
	var saves []bool
	checks := 0
	s.ComputerActions = &settings.ComputerActions{
		Save:              func() { saves = append(saves, s.Computer.Enabled.Draft) },
		CheckPermissions:  func() { checks++ },
		RequestPermission: func(resource string) { requests = append(requests, resource) },
		OpenSettings:      func(string) { t.Fatal("opened unrelated system settings") },
	}
	view := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, catalog)
		settings.Page(c, s, nil, settings.Look{})
	}, 920, 640, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	view.SetLocale("en")
	view.Frame()
	if _, ok := view.AccessByRole(ui.RoleButton, en("permission_request")); ok || !view.HasText(en("computer_permissions_remote")) {
		t.Fatalf("remote permissions were not read-only: %q", view.Texts())
	}
	click := func(role ui.Role, label string) {
		t.Helper()
		x, ok := view.AccessByRole(role, label)
		if !ok || x.Disabled {
			t.Fatalf("no enabled %q: %q", label, view.Texts())
		}
		view.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
		view.Frame()
	}
	click(ui.RoleButton, en("computer_permissions_refresh"))
	click(ui.RoleSwitch, en("computer_enabled"))
	if checks != 1 || len(requests) != 0 || !slices.Equal(saves, []bool{false}) || s.Computer.Enabled.Revision != 7 {
		t.Fatalf("unavailable helper must still allow disabling a saved setting: checks %d, requests %v, saves %v", checks, requests, saves)
	}
	if x, ok := view.AccessByRole(ui.RoleSwitch, en("computer_enabled")); !ok || !x.Disabled {
		t.Fatal("the unavailable helper could be turned back on")
	}
	s.Computer.HostReadOnly, s.Computer.Available = false, true
	view.Frame()
	click(ui.RoleButton, en("permission_request"))
	if !slices.Equal(requests, []string{"permission_accessibility"}) {
		t.Fatalf("requests = %v", requests)
	}
}
