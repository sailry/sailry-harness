package settings

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// The help with Show app stays while any grant made by adding the
// application to a list is missing, not only full disk access.
func TestPermissionsShowAppHelp(t *testing.T) {
	help := i18n.In("en", "permission_disk_help")
	shows := func(list ...preview.Permission) bool {
		m := NewModel(preview.SettingsData())
		m.Permissions = list
		tt := ui.NewTester(func(c *ui.Context) { permissions(c, m) }, 900, 700)
		tt.SetLocale("en")
		tt.Frame()
		return tt.HasText(help)
	}
	disk := preview.Permission{Resource: "permission_full_disk", Status: "permission_granted", Settings: true}
	if !shows(disk, preview.Permission{Resource: "permission_accessibility", Status: "permission_partial"}) {
		t.Error("no help while accessibility is partly granted")
	}
	if !shows(disk, preview.Permission{Resource: "permission_screen", Status: "permission_denied"}) {
		t.Error("no help while screen recording is missing")
	}
	if shows(disk, preview.Permission{Resource: "permission_microphone", Status: "permission_denied"}) {
		t.Error("help for the microphone, which the system asks for")
	}
	if shows(preview.Permission{Resource: "permission_full_disk", Status: "permission_remote"}, preview.Permission{Resource: "permission_screen", Status: "permission_granted"}) {
		t.Error("help with every list grant made or not made here")
	}
}
