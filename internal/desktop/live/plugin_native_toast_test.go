package live

import (
	"encoding/json"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
)

func TestNativeToastsExpireAndKeepActions(t *testing.T) {
	for _, kind := range []string{"info", "error"} {
		t.Run(kind, func(t *testing.T) {
			root := &Runtime{ctx: t.Context()}
			native := plugins.NewNativeBinding(t.Context())
			state := &pluginNativeState{runtime: root, native: &native, toasts: map[string]uint64{}, toastEvents: make(chan json.RawMessage, 16)}
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
					if item.Role == ui.RoleButton && item.Name != "Try again" {
						t.Fatalf("unexpected plugin toast button: %+v", item)
					}
				}
			}
			show()
			window.Advance(6 * time.Second)
			if window.HasText("Plugin toast fixture") || len(state.toasts) != 0 || len(state.toastEvents) != 0 {
				t.Fatalf("expiry left a toast or emitted an action: tracked %v, events %d, text %q", state.toasts, len(state.toastEvents), window.Texts())
			}
			show()
			if err := window.Click("Try again"); err != nil {
				t.Fatal(err)
			}
			window.Advance(time.Second)
			if window.HasText("Plugin toast fixture") || len(state.toasts) != 0 || len(state.toastEvents) != 1 {
				t.Fatalf("the action did not dismiss once: tracked %v, events %d, text %q", state.toasts, len(state.toastEvents), window.Texts())
			}
			var event map[string]string
			if err := json.Unmarshal(<-state.toastEvents, &event); err != nil || event["id"] != "fixture" || event["action"] != "retry" {
				t.Fatalf("the action lost its plugin identity: %v, %v", event, err)
			}
		})
	}
}
