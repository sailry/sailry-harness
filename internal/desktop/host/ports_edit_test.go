package host

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestPortEditUsesReplacementAndKeepsFailedDraft(t *testing.T) {
	previous := Port{Remote: 8080, Local: 18080, Listening: true}
	host := &Host{Ports: []Port{previous}}
	state := &State{ports: portForm{open: true, editing: &previous, remote: "8081", local: "18080"}}
	var complete func(bool)
	replacements := 0
	actions := Actions{
		CreatePort: func(string, string, func(bool)) { t.Error("editing created a separate forward") },
		ClosePort:  func(Port) { t.Error("editing deleted the old forward before replacement") },
		ReplacePort: func(old Port, remote, local string, completed func(bool)) {
			if old != previous || remote != "8081" || local != "18080" {
				t.Errorf("replacement source or draft changed: %+v, %s, %s", old, remote, local)
			}
			replacements++
			complete = completed
		},
	}
	tester := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		portDialogs(c, state, host, actions)
	}, 600, 400, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	tester.Frame()
	tester.Frame()
	if err := tester.Click(i18n.In("en", "settings_save")); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if replacements != 1 || !state.ports.pending || complete == nil {
		t.Fatal("save did not start one replacement")
	}
	complete(false)
	tester.Frame()
	if !state.ports.open || state.ports.pending || state.ports.remote != "8081" || state.ports.local != "18080" || len(host.Ports) != 1 || host.Ports[0] != previous {
		t.Fatal("failed replacement lost the original mapping or draft")
	}
	if err := tester.Click(i18n.In("en", "settings_save")); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	complete(true)
	tester.Frame()
	if replacements != 2 || state.ports.open || state.ports.pending {
		t.Fatal("successful replacement did not close the retained draft")
	}
}
