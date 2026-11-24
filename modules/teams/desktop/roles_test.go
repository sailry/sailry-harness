package desktop

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// The roles a new Node starts with are roles as any other: each can be
// edited and deleted.
func TestPresetRolesEditable(t *testing.T) {
	en := func(key string) string { return testCatalog.In("en", key) }
	state := settings.New()
	state.SetNativePages([]settings.NativePage{Page()})
	state.Open(settings.Teams)
	m := pageFor(state.Model)
	m.tab = 1
	tt := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		settings.Page(c, state, nil, settings.Look{})
	}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
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

func TestRolePresetSelection(t *testing.T) {
	m := pageFor(settings.New().Model)
	m.roleEdit = roleEditor{open: true, role: viewmodel.Role{Model: "chosen/model", Effort: "high"}}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		roleDialog(c, m)
	}, 490, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	settle := func() { tt.Frame(); tt.Advance(time.Second); tt.Frame() }
	settle()
	var firstY, lastY float32
	selectPreset := func(i int) {
		t.Helper()
		pr := rolePresets[i]
		label := testCatalog.In("en", pr.label)
		button, ok := tt.AccessByRole(ui.RoleButton, label)
		if !ok {
			t.Fatalf("missing preset %s", label)
		}
		if i == 0 {
			firstY = button.Bounds.Y
		}
		lastY = button.Bounds.Y
		tt.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
		settle()
		role := m.roleEdit.role
		if role.Name != label || role.Description != testCatalog.In("en", pr.description) || role.Instructions != testCatalog.In("en", pr.prompt) || role.Model != "chosen/model" || role.Effort != "high" {
			t.Fatalf("preset %s did not populate its role or changed its model", pr.key)
		}
		if _, ok := tt.Find(testCatalog.In("en", pr.modelHint)); !ok {
			t.Fatalf("missing model guidance for %s", pr.key)
		}
	}
	for i, key := range []string{"explore", "plan", "implement", "frontend", "test", "review", "troubleshoot"} {
		selectPreset(i)
		wantReadOnly := key == "explore" || key == "plan" || key == "review"
		if m.roleEdit.role.ReadOnly != wantReadOnly {
			t.Fatalf("unexpected default permissions for %s: got %v, want %v", key, m.roleEdit.role.ReadOnly, wantReadOnly)
		}
		if m.roleEdit.role.Key != key {
			t.Fatalf("preset key = %s, want %s", m.roleEdit.role.Key, key)
		}
	}
	if firstY >= lastY {
		t.Fatal("preset buttons did not wrap in a narrow dialog")
	}
	toggle, ok := tt.AccessByRole(ui.RoleSwitch, testCatalog.In("en", "teams_read_only"))
	if !ok {
		t.Fatal("missing read-only switch")
	}
	tt.ClickAt(toggle.Bounds.X+toggle.Bounds.W/2, toggle.Bounds.Y+toggle.Bounds.H/2)
	settle()
	m.roleEdit.role.Key = "custom"
	selectPreset(2) // writable preset must not override manual read-only toggle
	if m.roleEdit.role.Key != "custom" || !m.roleEdit.role.ReadOnly {
		t.Fatal("selecting another preset replaced the custom key or permissions")
	}

	for _, saved := range []struct {
		key, hint string
		readOnly  bool
	}{
		{"explore", "role_model_hint_explore", true},
		{"research", "role_model_hint_research", true},
		{"review", "role_model_hint_review", true},
	} {
		m.roleEdit = roleEditor{open: true, role: viewmodel.Role{ID: "saved-" + saved.key, Key: saved.key, ReadOnly: saved.readOnly}}
		settle()
		if _, ok := tt.Find(testCatalog.In("en", saved.hint)); !ok || m.roleEdit.role.ReadOnly != saved.readOnly {
			t.Fatalf("saved %s role lost its model guidance or permissions", saved.key)
		}
	}

	existing := viewmodel.Role{ID: "legacy-docs", Key: "docs", Name: "Documentation", Instructions: "Keep project docs current"}
	m.Roles = append(m.Roles, existing)
	m.roleEdit = roleEditor{open: true, role: existing}
	settle()
	if _, ok := tt.AccessByRole(ui.RoleButton, "Documentation"); ok {
		t.Fatal("saved role showed a documentation preset")
	}
	m.roleEdit.role.Name = "Project documentation"
	save, ok := tt.AccessByRole(ui.RoleButton, testCatalog.In("en", "role_save"))
	if !ok {
		t.Fatal("missing save action")
	}
	tt.ClickAt(save.Bounds.X+save.Bounds.W/2, save.Bounds.Y+save.Bounds.H/2)
	settle()
	if got := m.role(existing.ID); got == nil || got.Key != existing.Key || got.Instructions != existing.Instructions || got.Name != "Project documentation" || m.roleEdit.open {
		t.Fatal("legacy documentation role did not save its edits")
	}
}
