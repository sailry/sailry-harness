package live

import (
	"log/slog"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/node/logging"
)

func TestNativeToastsExpireAndIgnoreActions(t *testing.T) {
	for _, kind := range []string{"info", "error"} {
		t.Run(kind, func(t *testing.T) {
			root := &Runtime{ctx: t.Context()}
			native := plugins.NewNativeBinding(t.Context())
			state := &pluginNativeState{runtime: root, native: &native, toasts: map[string]uint64{}}
			window := closeTestWindow(root)
			show := func() {
				t.Helper()
				_, err := state.showToast(t.Context(), pluginLocationTestArguments(t, map[string]any{
					"id": "fixture", "message": "Plugin toast fixture", "kind": kind,
					"action": map[string]string{"id": "retry", "label": "Try again"},
				}))
				if err != nil {
					t.Fatal(err)
				}
				window.Frame()
				window.Advance(500 * time.Millisecond)
				if !window.HasText("Plugin toast fixture") || len(state.toasts) != 1 {
					t.Fatalf("the plugin toast was not shown: tracked %v, text %q", state.toasts, window.Texts())
				}
				for _, item := range window.AccessAll() {
					if item.Role == ui.RoleButton && item.Name == "Try again" {
						t.Fatalf("unexpected plugin toast button: %+v", item)
					}
				}
			}
			show()
			window.Advance(6 * time.Second)
			if window.HasText("Plugin toast fixture") || len(state.toasts) != 0 {
				t.Fatalf("expiry left a toast: tracked %v, text %q", state.toasts, window.Texts())
			}
			show()
			if _, err := state.dismissToast(t.Context(), pluginLocationTestArguments(t, "fixture")); err != nil {
				t.Fatal(err)
			}
			window.Frame()
			window.Advance(time.Second)
			if window.HasText("Plugin toast fixture") || len(state.toasts) != 0 {
				t.Fatalf("dismissal left a toast: tracked %v, text %q", state.toasts, window.Texts())
			}
		})
	}
}

// An error toast records the plugin's failure under its id with the code it
// gives; the message is not recorded, and a malformed code is refused.
func TestNativeErrorToastRecordsFailure(t *testing.T) {
	closeLog, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "desktop")
	if err != nil {
		t.Fatal(err)
	}
	defer closeLog()
	var severity slog.Level
	var fields logging.Fields
	logging.SetReport(func(level slog.Level, values logging.Fields) { severity, fields = level, values })
	defer logging.SetReport(nil)
	root := &Runtime{ctx: t.Context()}
	native := plugins.NewNativeBinding(t.Context())
	state := &pluginNativeState{runtime: root, native: &native, toasts: map[string]uint64{}}
	state.scope.Package.Name = "git"
	if _, err := state.showToast(t.Context(), pluginLocationTestArguments(t, map[string]any{
		"id": "git-error", "message": "Git failed", "kind": "error", "code": "not_configured",
	})); err != nil {
		t.Fatal(err)
	}
	if severity != slog.LevelError || fields["area"] != "plugin" || fields["operation"] != "git-error" || fields["code"] != "not_configured" || fields["package"] != "git" {
		t.Fatalf("level %v, fields %v", severity, fields)
	}
	for _, invalid := range []map[string]any{
		{"id": "bad", "message": "Failed", "kind": "error", "code": "Not Configured"},
		{"id": "bad", "message": "Done", "kind": "info", "code": "unavailable"},
	} {
		if _, err := state.showToast(t.Context(), pluginLocationTestArguments(t, invalid)); err == nil {
			t.Fatalf("accepted %v", invalid)
		}
	}
}
