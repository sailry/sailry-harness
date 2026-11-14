package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProjectCallRespectsDeclaredDisclosure(t *testing.T) {
	for _, expandable := range []bool{false, true} {
		summary := protocol.DefaultToolDetailSummary()
		summary.HasDetails = expandable
		summary.Resolved = &protocol.ToolDetailResolved{
			Label: &protocol.ToolResultLocalText{Label: "Read file"},
			Input: protocol.ToolDetailInput{Summary: &protocol.ToolDetailText{Text: "input.go", Code: true}},
			// Older projections may include a preview even when HasDetails is false.
			Output: &protocol.ToolDetailOutput{Preview: "package fixture", Paths: []string{"input.go"}},
		}
		call := projectCall("read_file", &summary, "en")
		if call.Label != "Read file" || call.Detail != "input.go" || !call.Code {
			t.Fatalf("input identity changed: %+v", call)
		}
		if !expandable {
			if call.Output != nil {
				t.Fatalf("summary-only tool acquired result disclosure: %+v", call.Output)
			}
		} else if call.Output == nil || call.Output.Text != "package fixture" || len(call.Output.Paths) != 1 || call.Output.Paths[0] != "input.go" {
			t.Fatalf("declared details lost output: %+v", call.Output)
		}
	}
}
