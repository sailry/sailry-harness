package kit

import (
	"github.com/egoist/mygo/plugins/loading"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Thinking is the thinking indicator's state: what the run does picks its
// mark.
type Thinking int

const (
	// Breathing is the thinking mark held still: no known phase.
	Breathing Thinking = iota
	// Working: tools or subagents run (the loader's mark).
	Working
	// Searching: the model thinks or the request waits.
	Searching
	// Composing: the model writes the answer.
	Composing
	// Waiting: the run is queued behind another.
	Waiting
)

// ThinkingOf maps a phase's label key to its state (ui/thinking state).
func ThinkingOf(phase string) Thinking {
	switch phase {
	case "turn_awaiting_response", "turn_thinking":
		return Searching
	case "turn_generating":
		return Composing
	case "turn_tools_running", "turn_waiting_children", "turn_waiting_child":
		return Working
	case "turn_queued":
		return Waiting
	}
	return Breathing
}

// Indicator is the mark of what a running turn does, in a box size across,
// in the muted color of the phase's text beside it, of loading.dev's marks
// (MyGo's plugins/loading, and marks.go for the two it lacks): a dash going
// round a figure of eight while the model thinks or the request waits
// (Loop); three chevrons lighting in turn while it writes (Chevrons); the
// loader's mark while tools or subagents run (loading.Swirl); a dash
// tracing a rounded square while the run is queued (loading.Trace); a dot
// held still for no known phase (loading.Pulse, paused).
func Indicator(c *ui.Context, st Thinking, size float32) ui.Element {
	o := loading.Options{Size: size, Color: theme.Of(c).MutedForeground}
	switch st {
	case Working:
		return loading.Swirl(c, o)
	case Searching:
		return Loop(c, size, o.Color)
	case Composing:
		return Chevrons(c, size, o.Color)
	case Waiting:
		return loading.Trace(c, o)
	}
	o.Paused = true
	return loading.Pulse(c, o)
}
