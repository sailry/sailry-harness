package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/identity"
)

func TestHostUpdateUsesProductTargetWithoutDowngrading(t *testing.T) {
	for _, row := range []struct {
		current, target string
		available       bool
	}{
		{"0.1.0", "0.1.1", true},
		{"0.1.0", "0.1.0", false},
		{"0.2.13", "0.1.0", false},
		{"0.1.0", "", false},
		{"unknown", "0.1.1", false},
		{"0.1.0", "unknown", false},
	} {
		if got := hostUpdateAvailable(row.current, row.target); got != row.available {
			t.Errorf("update %s to %s = %v, want %v", row.current, row.target, got, row.available)
		}
	}
	root := &Runtime{updates: &Updates{hostRelease: identity.CodeHost()}}
	state := &liveHost{view: &host.Host{Runtime: host.Runtime{Version: "0.1.0"}, Update: &host.Update{}}}
	if root.hostUpdateAvailable(state) {
		t.Fatal("an unchecked Code Host used the Harness build as its target")
	}
	state.view.Update.Target = "0.1.1"
	if !root.hostUpdateAvailable(state) {
		t.Fatal("the Code Host ignored its resolved product target")
	}
	state.view.Runtime.Version = "0.2.13"
	if root.hostUpdateAvailable(state) {
		t.Fatal("a legacy Harness Host was offered a lower Code release")
	}
}
