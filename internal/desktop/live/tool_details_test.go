package live

import (
	"encoding/json"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProjectToolDetailKeepsCanonicalStateAndImages(t *testing.T) {
	for _, test := range []struct {
		name    string
		state   conversation.RowState
		pending bool
		failed  bool
	}{
		{"runtime_failure", conversation.StateNone, false, true},
		{"denied", conversation.Denied, false, false},
		{"interrupted", conversation.StateInterrupted, false, false},
		{"pending", conversation.StateNone, true, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			background := &conversation.Background{Running: true}
			image := conversation.Image{ID: "fixture-image", Name: "fixture.png", Width: 4, Height: 4}
			call := conversation.Call{ID: "fixture-call", Name: "run_command", State: test.state, Pending: test.pending,
				Running: true, Standalone: true, Terminal: true, Background: background, Output: &conversation.Output{Images: []conversation.Image{image}}}
			summary := protocol.DefaultToolDetailSummary()
			summary.HasDetails = true
			summary.Failure = &protocol.ToolFailure{Origin: protocol.ToolFailureOriginRuntime}
			raw := json.RawMessage(`{"kind":"command_result","stdout":"fixture"}`)
			loaded := protocol.DefaultToolDetail()
			loaded.Result = &raw
			loaded.Resolved = &protocol.ToolDetailResolved{
				Label:  &protocol.ToolResultLocalText{Label: "Fixture command"},
				Input:  protocol.ToolDetailInput{Summary: &protocol.ToolDetailText{Text: "printf fixture", Code: true}},
				Output: &protocol.ToolDetailOutput{Preview: "fixture", Paths: []string{"fixture.txt"}},
			}
			ProjectToolDetail(&call, &summary, loaded, "en")
			if call.ID != "fixture-call" || call.Name != "run_command" || !call.Standalone || !call.Terminal || call.Background != background || call.Pending != test.pending {
				t.Fatal("loaded details changed canonical identity or interaction state")
			}
			if call.Label != "Fixture command" || call.Detail != "printf fixture" || !call.Code || call.Output == nil || call.Output.Text != "fixture" || !call.Output.Command || !reflect.DeepEqual(call.Output.Paths, []string{"fixture.txt"}) || !reflect.DeepEqual(call.Output.Images, []conversation.Image{image}) {
				t.Fatalf("loaded content was not projected consistently: %+v", call)
			}
			if call.Failed != test.failed || summary.Resolved != nil {
				t.Fatal("projection lost runtime failure state or mutated the canonical summary")
			}
			if test.pending {
				if call.State != conversation.Pending || call.Running {
					t.Fatal("loaded details overrode the pending approval")
				}
			} else if !test.failed && (call.State != test.state || !call.Warning || call.Running) {
				t.Fatal("loaded details overrode a denied or interrupted call")
			}
		})
	}
}
