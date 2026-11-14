package settings

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// The roles a new Node starts with are roles as any other: each can be
// edited and deleted.
func TestPresetRolesEditable(t *testing.T) {
	en := func(key string) string { return i18n.In("en", key) }
	m := NewModel(preview.SettingsData())
	m.TeamTab = 1
	tt := ui.NewTesterWith(func(c *ui.Context) { teams(c, m) }, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	settle := func() { tt.Frame(); tt.Advance(time.Second); tt.Frame() }
	settle()
	deletes := 0
	for _, a := range tt.AccessAll() {
		if a.Role == ui.RoleButton && a.Name == en("settings_delete") {
			deletes++
			if a.Disabled {
				t.Errorf("a delete button at %v is disabled", a.Bounds)
			}
		}
	}
	if deletes != len(m.Roles) {
		t.Fatalf("%d delete buttons for %d roles", deletes, len(m.Roles))
	}
	m.roleEdit = roleEditor{open: true, role: *m.role("explore")}
	settle()
	for _, a := range tt.AccessAll() {
		if (a.Name == en("settings_name") || a.Name == en("role_id")) && a.Disabled {
			t.Errorf("the preset role's %q is locked", a.Name)
		}
	}
}
