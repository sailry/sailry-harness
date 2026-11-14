package live

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConversationSyncFailureKeepsUnknownRunWithoutLiveActivity(t *testing.T) {
	for _, kind := range []protocol.ConversationRunKind{protocol.ConversationRunKindTask, protocol.ConversationRunKindCompaction} {
		t.Run(string(kind), func(t *testing.T) {
			turn := protocol.TurnID{1}
			started := int64(1)
			page := protocol.DefaultConversationPage()
			page.Runs = []protocol.ConversationRun{{Turn: turn, Kind: kind, Status: protocol.ConversationStatusRunning, StartedMs: &started}}
			callID := "read"
			page.Entries = []protocol.ConversationEntry{{ID: "call", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{
				{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{ID: &callID, Name: "read_file"}},
				{Kind: "resource", Data: json.RawMessage(`{"type":"tool_started","id":"read","name":"read_file"}`)},
			}}}
			state := client.ConversationState{Snapshot: protocol.ConversationSnapshot{Page: page}, Err: &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "run belongs to another conversation"}}}
			turns, session, _, _ := Conversation(state)
			if len(turns) != 1 || turns[0].Reply.Status != conversation.Unsynced || turns[0].Reply.Elapsed != 0 || session.Loading || session.Connection != conversation.Connected || session.Diagnostic == nil {
				t.Fatalf("topic failure became a running animation or Host disconnect: turns=%+v, session=%+v", turns, session)
			}
			if kind == protocol.ConversationRunKindCompaction && turns[0].Compaction.State != conversation.CompactionUnsynced {
				t.Fatal("stale compaction kept running", turns[0].Compaction)
			}
			if calls := turns[0].Reply.Blocks[0].Calls; len(calls) != 1 || calls[0].Running {
				t.Fatal("stale tool call kept running", calls)
			}
			if page.Runs[0].Status != protocol.ConversationStatusRunning {
				t.Fatal("presentation changed the last accepted run status")
			}
			state.Connected, state.Err = true, nil
			resumed, restored, _, _ := Conversation(state)
			if resumed[0].Reply.Status != conversation.Running || restored.Diagnostic != nil || !resumed[0].Reply.Blocks[0].Calls[0].Running {
				t.Fatal("valid observation did not resume live activity", resumed, restored)
			}
			if kind == protocol.ConversationRunKindCompaction && resumed[0].Compaction.State != conversation.CompactionRunning {
				t.Fatal("valid compaction observation did not resume", resumed[0].Compaction)
			}
		})
	}
}

func TestConversationSyncFailureFreezesChildrenWithoutChangingRunStatus(t *testing.T) {
	parent, childID := protocol.SessionID{1}, protocol.SessionID{2}
	turn := protocol.TurnID{1}
	page := protocol.DefaultConversationPage()
	page.Session = parent
	page.Runs = []protocol.ConversationRun{{Session: parent, Turn: turn, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning}}
	page.Children = []protocol.ConversationChild{{
		Run:    protocol.ConversationRun{Session: childID, Turn: protocol.TurnID{2}, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning},
		Origin: protocol.Delegation{Session: parent, Turn: turn},
	}}
	for _, connected := range []bool{false, true} {
		turns, _, _, _ := Conversation(client.ConversationState{Connected: connected, Snapshot: protocol.ConversationSnapshot{Page: page}})
		var children []conversation.Child
		for _, turn := range turns {
			for _, block := range turn.Reply.Blocks {
				children = append(children, block.Children...)
			}
		}
		if len(children) != 1 || children[0].Status != conversation.ChildRunning || children[0].Unsynced == connected {
			t.Fatalf("connected=%v: children=%+v", connected, children)
		}
	}
	if page.Children[0].Run.Status != protocol.ConversationStatusRunning {
		t.Fatal("presentation changed the child's last accepted status")
	}
}
