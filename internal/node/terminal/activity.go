package terminal

import (
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// activityTracker reads what a program reports about itself in its output:
// progress (OSC 9;4) and the shell's command marks (OSC 133 and OSC 633).
// A sequence may be split across reads.
type activityTracker struct {
	state byte // 0 outside a sequence, 1 after ESC, 2 inside an OSC
	// body is the start of the current OSC; no report is longer.
	body     []byte
	activity protocol.TerminalActivity
}

// update consumes output and returns the activity when it changed.
func (tracker *activityTracker) update(output []byte) (protocol.TerminalActivity, bool) {
	before := tracker.activity
	for _, value := range output {
		switch {
		case value == 0x18 || value == 0x1a:
			// CAN and SUB cancel a sequence without reporting it.
			tracker.state = 0
		case value == 0x1b:
			// ESC ends an OSC, as the first byte of its terminator does.
			if tracker.state == 2 {
				tracker.report()
			}
			tracker.state = 1
		case tracker.state == 1:
			// Other control characters leave the escape pending.
			if value == ']' {
				tracker.state, tracker.body = 2, tracker.body[:0]
			} else if value >= 0x20 && value != 0x7f {
				tracker.state = 0
			}
		case tracker.state == 2 && value == 0x07:
			tracker.report()
			tracker.state = 0
		case tracker.state == 2 && value >= 0x20 && len(tracker.body) < 8:
			tracker.body = append(tracker.body, value)
		}
	}
	return tracker.activity, tracker.activity != before
}

func (tracker *activityTracker) report() {
	kind, rest, _ := strings.Cut(string(tracker.body), ";")
	value, rest, more := strings.Cut(rest, ";")
	switch kind {
	case "9":
		if state, _, _ := strings.Cut(rest, ";"); value == "4" && more {
			switch state {
			case "0":
				tracker.activity = protocol.TerminalActivityIdle
			case "1", "3":
				tracker.activity = protocol.TerminalActivityWorking
			case "2":
				tracker.activity = protocol.TerminalActivityFailed
			case "4":
				tracker.activity = protocol.TerminalActivityWaitingForInput
			}
		}
	case "133", "633":
		// Command marks bound a command's lifetime, which may be spent
		// awaiting input: they clear stale progress without implying work.
		switch value {
		case "A", "B", "C", "D":
			tracker.activity = protocol.TerminalActivityIdle
		}
	}
}
