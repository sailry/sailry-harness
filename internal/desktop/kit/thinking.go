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
	}
	return Breathing
}

// Indicator is the mark of what a running turn does, in a box size across,
// in the muted color of the phase's text beside it, of MyGo's loading marks
// (loading.go): a dot whose ring ripples outward while the model thinks or
// the request waits (loading.Pulse), held still for no known phase; three
// dots lighting in turn while it writes (loading.LinearDots, narrower than
// high, in the middle of the box); the loader's mark while tools or
// subagents run (loading.Swirl).
func Indicator(c *ui.Context, st Thinking, size float32) ui.Element {
	o := loading.Options{Size: size, Color: theme.Of(c).MutedForeground}
	switch st {
	case Working:
		return loading.Swirl(c, o)
	case Searching:
		return loading.Pulse(c, o)
	case Composing:
		return loading.LinearDots(c, o).Width(size)
	}
	o.Paused = true
	return loading.Pulse(c, o)
}
