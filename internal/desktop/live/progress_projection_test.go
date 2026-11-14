package live

import (
	"encoding/json"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConversationProgressResults(t *testing.T) {
	for _, row := range []struct {
		name   string
		status protocol.ConversationStatus
	}{
		{"update_plan", "running"},
		{"update_plan", "completed"},
		{"plugin_project_checklist", "running"},
		{"plugin_project_checklist", "completed"},
	} {
		t.Run(row.name+"/"+string(row.status), func(t *testing.T) {
			session, turn := protocol.NewSessionID(), protocol.NewTurnID()
			callID := "plan"
			page := protocol.DefaultConversationPage()
			page.Session = session
			page.Runs = []protocol.ConversationRun{{Session: session, Worktree: protocol.WorktreeID{1}, Turn: turn, Kind: "task", Status: row.status, Sequence: 1, Revision: 1}}
			page.Entries = []protocol.ConversationEntry{{ID: "request", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{
				{Kind: "text", Data: "Before the plan"},
				{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{ID: &callID, Name: row.name, Presentation: "progress", Grouping: "standalone", Summary: &protocol.ToolDetailSummary{}}},
				{Kind: "text", Data: "After the plan"},
			}}}
			project := func() []conversation.Block {
				t.Helper()
				turns, _, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}})
				if len(turns) != 1 {
					t.Fatalf("turns = %#v", turns)
				}
				wire, err := json.Marshal(protocol.ConversationSnapshot{Node: protocol.NodeID{1}, Page: page})
				if err != nil {
					t.Fatal(err)
				}
				var replay protocol.ConversationSnapshot
				if err := json.Unmarshal(wire, &replay); err != nil {
					t.Fatal(err)
				}
				replayed, _, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: replay})
				if len(replayed) != 1 || !reflect.DeepEqual(turns[0].Reply.Blocks, replayed[0].Reply.Blocks) {
					t.Fatalf("history replay changed typed progress: %#v", replayed)
				}
				return turns[0].Reply.Blocks
			}
			if blocks := project(); len(blocks) != 3 || blocks[1].Kind != conversation.ToolsBlock {
				t.Fatalf("pending plan = %#v", blocks)
			}
			page.Entries = append(page.Entries, protocol.ConversationEntry{ID: "result", Turn: turn, Author: "tool", Parts: []protocol.ConversationPart{
				{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{ID: &callID, Name: row.name, Summary: &protocol.ToolDetailSummary{Progress: &protocol.ConversationProgress{Steps: []protocol.ConversationProgressStep{
					{Description: "Inspect", State: "completed"},
					{Description: "Fix", State: "in_progress"},
				}}}}},
			}})
			blocks := project()
			if len(blocks) != 3 || blocks[0].Text != "Before the plan" || blocks[2].Text != "After the plan" {
				t.Fatalf("plan changed transcript order = %#v", blocks)
			}
			plan := blocks[1]
			if plan.Kind != conversation.ProgressBlock || len(plan.Calls) != 0 || len(plan.Steps) != 2 || plan.Steps[0].Text != "Inspect" || plan.Steps[0].State != conversation.StepCompleted || plan.Steps[1].State != conversation.StepInProgress {
				t.Fatalf("validated plan was not projected = %#v", plan)
			}
			// Subsequent results update the same plan component used by the composer.
			nextID := "next-plan"
			page.Entries = append(page.Entries,
				protocol.ConversationEntry{ID: "next-request", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{ID: &nextID, Name: row.name, Presentation: "progress", Grouping: "standalone", Summary: &protocol.ToolDetailSummary{}}}}},
				protocol.ConversationEntry{ID: "next-result", Turn: turn, Author: "tool", Parts: []protocol.ConversationPart{{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{ID: &nextID, Name: row.name, Summary: &protocol.ToolDetailSummary{Progress: &protocol.ConversationProgress{Steps: []protocol.ConversationProgressStep{{Description: "Fix", State: "completed"}}}}}}}},
			)
			if blocks = project(); len(blocks) != 3 || blocks[1].Kind != conversation.ProgressBlock || len(blocks[1].Steps) != 1 || blocks[1].Steps[0].Text != "Fix" || blocks[1].Steps[0].State != conversation.StepCompleted {
				t.Fatalf("latest plan was not retained = %#v", blocks)
			}
		})
	}
}
