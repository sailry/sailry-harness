package composer_test

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

type stopper struct {
	composer.Actions
	stopped int
}

func (s *stopper) Stop() { s.stopped++ }

// Escape stops what the Stop button stops: the subagents a finished reply
// left running, while there is no draft to send.
func TestEscapeStopsWhatTheButtonStops(t *testing.T) {
	state := composer.New(preview.DemoModel)
	state.Stoppable = true
	actions := &stopper{}
	if !state.Stops() || !state.Escapes() || !state.Escape(actions) || actions.stopped != 1 {
		t.Fatalf("Escape with subagents running: stops %v, stopped %d", state.Stops(), actions.stopped)
	}
	state.Draft = "next message"
	if state.Stops() || state.Escapes() || state.Escape(actions) || actions.stopped != 1 {
		t.Fatalf("Escape over a draft stopped the subagents: stopped %d", actions.stopped)
	}
}
