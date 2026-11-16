package shell

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestComposerKeepsExecutionSeparateFromMonitor(t *testing.T) {
	data := &viewmodel.Data{Workspace: &viewmodel.Workspace{Hosts: []viewmodel.Host{{ID: "local", Local: true}, {ID: "remote"}}},
		Unassigned: []*viewmodel.Conversation{{ID: "draft/new"}, {ID: "captured", Node: "remote"}}}
	app := NewLive(data)
	app.Host = "remote"
	if got := app.Composer("draft/new").Host; got != "local" {
		t.Fatalf("projectless draft followed monitor: %q", got)
	}
	if got := app.Composer("captured").Host; got != "remote" {
		t.Fatalf("existing conversation lost captured execution host: %q", got)
	}
}
