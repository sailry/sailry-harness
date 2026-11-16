package settings

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestConnectionsDeviceMonitor(t *testing.T) {
	for _, device := range []viewmodel.Device{
		{ID: "local", Name: "Local computer", Execution: true, Local: true, Online: true},
		{ID: "remote", Name: "Remote computer", Execution: true},
		{ID: "phone", Name: "Mobile controller"},
	} {
		t.Run(device.ID, func(t *testing.T) {
			m := &Model{Devices: []viewmodel.Device{device}}
			inspected := ""
			m.ConnectionActions = &ConnectionActions{Inspect: func(value viewmodel.Device) { inspected = value.ID }}
			tt := ui.NewTester(func(c *ui.Context) {
				ui.Column(c).Fill().Padding(24).Children(func() { connections(c, m) })
			}, 900, 1000)
			tt.SetLocale("en")
			tt.Frame()
			info, ok := tt.AccessByRole(ui.RoleMenuButton, i18n.In("en", "more"))
			if !ok {
				t.Fatal("device has no More menu")
			}
			tt.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
			tt.Advance(250 * time.Millisecond)
			monitor, offered := tt.AccessByRole(ui.RoleMenuItem, i18n.In("en", "workspace_host"))
			if offered != device.Execution {
				t.Fatalf("monitor offered for execution=%v: %q", device.Execution, tt.Texts())
			}
			if !device.Execution {
				return
			}
			tt.ClickAt(monitor.Bounds.X+monitor.Bounds.W/2, monitor.Bounds.Y+monitor.Bounds.H/2)
			tt.Advance(250 * time.Millisecond)
			if inspected != device.ID {
				t.Fatalf("monitor targeted %q, want %q", inspected, device.ID)
			}
		})
	}
}
