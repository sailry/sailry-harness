package shell

import (
	"os"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// The host page's review sheets, written only when $SAILRY_SNAPSHOTS names a
// directory: a connected local host with running agents, a
// paired host that is offline and one with an update to install, each in
// both themes, in a window tall enough for the whole page, and the page in
// Demo mode. The preview's
// hosts stand in for live ones: marked live, they show a Node's own name and
// platform and none of the preview's notes.
func TestHostPageSnapshots(t *testing.T) {
	if os.Getenv("SAILRY_SNAPSHOTS") == "" {
		t.Skip("SAILRY_SNAPSHOTS is not set")
	}
	live := func(h *host.Host, name, detail string) {
		h.Live, h.Name, h.Detail = true, name, detail
		if h.Metrics != nil {
			for i := range h.Metrics.Processes {
				h.Metrics.Processes[i].Name = []string{"sailry-node", "rust-analyzer"}[i%2]
				h.Metrics.Processes[i].PID = uint32(4100 + 17*i)
			}
		}
	}
	states := []struct {
		name, id string
		prepare  func(*host.Host, *host.State)
	}{
		{"local", "local", func(h *host.Host, _ *host.State) {
			live(h, "sailry-mac.local", "macOS · 26.1 · arm64")
		}},
		{"offline", "remote", func(h *host.Host, _ *host.State) {
			live(h, "design-node.local", "Linux · 6.8 · x86_64")
			h.Runtime = host.Runtime{Uptime: -1, NodeUptime: -1, Error: "Host is disconnected"}
			h.Offline, h.Metrics, h.MetricsError, h.Update = true, nil, "Host is disconnected", nil
		}},
		{"update", "remote", func(h *host.Host, st *host.State) {
			live(h, "design-node.local", "Linux · 6.8 · x86_64")
			h.Runtime = host.Runtime{
				Version: "0.1.0", Uptime: 9*24*time.Hour + 3*time.Hour, NodeUptime: 26 * time.Hour,
				ProtocolMin: 1, ProtocolMax: 1, Known: true,
			}
			h.Update = &host.Update{Phase: host.Available, Current: "0.1.0", Target: "0.2.0", Progress: -1}
			st.OpenUpdate(h)
		}},
	}
	for _, state := range states {
		for _, mode := range []string{"light", "dark"} {
			a := New(preview.Load())
			tt := ui.NewTesterWith(a.View, 1440, 1960, opaque)
			tt.SetLocale("en")
			tt.SetDark(mode == "dark")
			a.openHost(state.id, false)
			tt.Frame()
			tt.Advance(2 * time.Second)
			tt.Frame()
			h, st := a.hostPage()
			state.prepare(h, st)
			tt.Frame()
			// Past the toast a failure raises, which would cover the header.
			tt.Advance(12 * time.Second)
			tt.Frame()
			tt.Advance(time.Second)
			tt.Frame()
			snapshot(t, tt, "host-"+state.name+"-"+mode)
		}
	}
	// Demo mode over the preview's hosts: the page says nowhere that its
	// data is a sample.
	for _, mode := range []string{"light", "dark"} {
		a := New(preview.Load())
		a.Settings.Model.DemoMode = true
		tt := ui.NewTesterWith(a.View, 1440, 1960, opaque)
		tt.SetLocale("en")
		tt.SetDark(mode == "dark")
		a.openHost("local", false)
		tt.Frame()
		tt.Advance(2 * time.Second)
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
		snapshot(t, tt, "host-demo-"+mode)
	}
	// The window as it opens, at twice the scale: the side column, the
	// header and the first cards.
	for _, mode := range []string{"light", "dark"} {
		a, tt := newTester(t)
		tt.SetScale(2)
		tt.SetDark(mode == "dark")
		a.openHost("local", false)
		tt.Frame()
		tt.Advance(2 * time.Second)
		tt.Frame()
		h, _ := a.hostPage()
		live(h, "sailry-mac.local", "macOS · 26.1 · arm64")
		tt.Frame()
		snapshot(t, tt, "host-window-"+mode)
		a.togglePanel()
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
		snapshot(t, tt, "host-summary-"+mode)
	}
}
