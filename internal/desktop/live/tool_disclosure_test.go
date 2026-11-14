package live

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
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

func TestHiddenToolFailuresRetainDetails(t *testing.T) {
	for _, origin := range []protocol.ToolFailureOrigin{"", protocol.ToolFailureOriginRuntime, protocol.ToolFailureOriginTool} {
		t.Run(string(origin), func(t *testing.T) {
			turn, id := protocol.NewTurnID(), "maintenance"
			summary := protocol.DefaultToolDetailSummary()
			if origin != "" {
				message := "original maintenance failure"
				summary.Error = &message
				summary.Failure = &protocol.ToolFailure{Origin: origin}
				summary.Diagnostics, summary.HasDetails = true, true
			}
			page := protocol.DefaultConversationPage()
			page.Session = protocol.NewSessionID()
			page.Runs = []protocol.ConversationRun{{Turn: turn, Kind: "task", Status: "completed"}}
			page.Entries = []protocol.ConversationEntry{
				{ID: "request", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{
					{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{ID: &id, Name: "maintenance", Presentation: protocol.ToolPresentationHidden, Grouping: protocol.ToolGroupingStandalone}},
					{Kind: "text", Data: "Retained answer"},
				}},
				{ID: "result", Turn: turn, Author: "tool", Parts: []protocol.ConversationPart{
					{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{ID: &id, Name: "maintenance", Summary: &summary}},
				}},
			}
			state := client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}}
			turns, _, _, _ := Conversation(state)
			if len(turns) != 1 || len(turns[0].Reply.Blocks) == 0 || turns[0].Reply.Blocks[0].Text != "Retained answer" {
				t.Fatalf("answer changed: %#v", turns)
			}
			if origin == "" {
				if len(turns[0].Reply.Blocks) != 1 {
					t.Fatalf("successful hidden tool became visible: %#v", turns[0].Reply.Blocks)
				}
				return
			}
			if len(turns[0].Reply.Blocks) != 2 || len(turns[0].Reply.Blocks[1].Calls) != 1 {
				t.Fatalf("hidden failure lost its row: %#v", turns[0].Reply.Blocks)
			}
			cv := &viewmodel.Conversation{Turns: turns}
			// An empty detail cache must bind the original request's identity to
			// the result's lazy loader, rather than quietly dropping the error.
			root := &Runtime{controller: new(client.Client)}
			root.enrichDetails(cv, state)
			call := &cv.Turns[0].Reply.Blocks[1].Calls[0]
			if call.ID != "request/0" || !call.Standalone || call.Load == nil || call.Output == nil || call.Error != *summary.Error {
				t.Fatalf("failure lost identity or detail access: %+v", call)
			}
			raw := json.RawMessage(`{"error":"original maintenance failure"}`)
			ProjectToolDetail(call, &summary, protocol.ToolDetail{Result: &raw})
			if call.Output == nil || call.Output.Text == "" || call.Failed != (origin == protocol.ToolFailureOriginRuntime) || call.Warning != (origin == protocol.ToolFailureOriginTool) {
				t.Fatalf("original failure output or status lost: %+v", call)
			}
		})
	}
}
