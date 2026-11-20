package conversation

import (
	"fmt"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type stillActions struct{ Actions }

// A run of calls keeps the transcript still: the phase row that shows
// between calls keeps its place while the next call runs, so a transcript
// resting on its end does not move once per call.
func TestToolRunKeepsTranscriptStill(t *testing.T) {
	call := func(id string, state int) Call {
		c := Call{ID: id, Name: "read_file", Label: "Read file", Detail: id + ".go"}
		switch state {
		case 0:
			c.AwaitingModel = true
		case 1:
			c.Running = true
		default:
			c.Output = &Output{Text: "ok"}
		}
		return c
	}
	var lines []string
	for i := range 80 {
		lines = append(lines, fmt.Sprintf("line %d", i))
	}
	var phase Phase
	var calls []Call
	s := New()
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		turns := []Turn{
			{ID: "earlier", User: "earlier", Reply: Reply{Status: Completed, Blocks: []Block{{Kind: TextBlock, Text: strings.Join(lines, "\n\n")}}}},
			{ID: "run", User: "go", Reply: Reply{Status: Running, Phase: phase, Blocks: []Block{{Kind: ToolsBlock, Calls: calls}}}},
		}
		ui.Column(c).Fill().Children(func() { View(c, s, "t", turns, &Session{}, stillActions{}) })
	}, 1000, 700)
	tt.SetLocale("en")
	var top float32 = -1
	done := []Call{call("a", 2)}
	for _, id := range []string{"b", "c", "d"} {
		for state, p := range []Phase{AwaitingResponse, ExecutingTool, AwaitingResponse} {
			phase, calls = p, append(append([]Call(nil), done...), call(id, state))
			tt.Frame()
			tt.Frame()
			user, ok := tt.Access("go")
			if !ok {
				t.Fatalf("the running turn is not shown: %q", tt.Texts())
			}
			if top >= 0 && user.Bounds.Y != top {
				t.Fatalf("call %s state %d moved the transcript from %.1f to %.1f", id, state, top, user.Bounds.Y)
			}
			top = user.Bounds.Y
		}
		done = append(done, call(id, 2))
	}
}

// Reasoning, waiting for a model, a question and a stop each show or hide the
// phase row; the transcript stays where it rests.
func TestPhaseChangesKeepTranscriptStill(t *testing.T) {
	var lines []string
	for i := range 80 {
		lines = append(lines, fmt.Sprintf("line %d", i))
	}
	reply := Reply{Status: Running, Blocks: []Block{{Kind: ReasoningBlock, Text: "Considering the options"}}}
	s := New()
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		turns := []Turn{
			{ID: "earlier", User: "earlier", Reply: Reply{Status: Completed, Blocks: []Block{{Kind: TextBlock, Text: strings.Join(lines, "\n\n")}}}},
			{ID: "run", User: "go", Reply: reply},
		}
		ui.Column(c).Fill().Children(func() { View(c, s, "t", turns, &Session{}, stillActions{}) })
	}, 1000, 700)
	tt.SetLocale("en")
	var top float32 = -1
	for _, step := range []struct {
		phase  Phase
		status RunStatus
	}{{Thinking, Running}, {AwaitingResponse, Running}, {WaitingUser, Running}, {Thinking, Running}, {ExecutingTool, Running}, {AwaitingResponse, Stopping}} {
		reply.Phase, reply.Status = step.phase, step.status
		tt.Frame()
		tt.Frame()
		user, ok := tt.Access("go")
		if !ok {
			t.Fatalf("the running turn is not shown: %q", tt.Texts())
		}
		if top >= 0 && user.Bounds.Y != top {
			t.Fatalf("phase %v (%v) moved the transcript from %.1f to %.1f", step.phase, step.status, top, user.Bounds.Y)
		}
		top = user.Bounds.Y
	}
}
