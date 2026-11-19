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
	page.Entries = []protocol.ConversationEntry{{ID: "launch", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{
		{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{Name: "spawn_agent", Summary: &protocol.ToolDetailSummary{}}},
	}}}
	page.Children = []protocol.ConversationChild{{
		Run:    protocol.ConversationRun{Session: childID, Turn: protocol.TurnID{2}, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning},
		Origin: protocol.Delegation{Session: parent, Turn: turn, Entry: "launch"},
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

func TestLiveToolDraftPairsOnlyWithItsBranch(t *testing.T) {
	node, session, turn := protocol.NodeID{1}, protocol.SessionID{1}, protocol.TurnID{1}
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Node, snapshot.Page.Session, snapshot.Page.Revision = node, session, 1
	snapshot.Page.Runs = []protocol.ConversationRun{{Session: session, Turn: turn, Sequence: 1, Revision: 1, Kind: "task", Status: "running"}}
	for index, branch := range []string{"", "other"} {
		snapshot.Page.Entries = append(snapshot.Page.Entries, protocol.ConversationEntry{ID: "call-" + branch, Turn: turn, Sequence: uint64(index + 1), Branch: branch, Author: "assistant", Parts: []protocol.ConversationPart{transcriptTool("shared", "read_file")}})
	}
	projection := client.NewConversationProjection(node, session, 1)
	if _, err := projection.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: snapshot}); err != nil {
		t.Fatal(err)
	}
	apply := func(kind string, data any) {
		t.Helper()
		snapshot.Sequence++
		frame := protocol.ConversationFrame{Node: node, Session: session, Sequence: snapshot.Sequence, Change: protocol.ConversationChange{Kind: kind, Data: data}}
		if outcome, err := projection.Apply(1, protocol.Update{Kind: "conversation_frame", Data: frame}); outcome != client.Applied || err != nil {
			t.Fatalf("apply %s: %v, %v", kind, outcome, err)
		}
	}
	check := func(active int) {
		t.Helper()
		turns, _, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: *projection.Snapshot()})
		if len(turns) != 1 || len(turns[0].Reply.Blocks) != 2 {
			t.Fatalf("unexpected transcript %+v", turns)
		}
		for index, block := range turns[0].Reply.Blocks {
			if len(block.Calls) != 1 {
				t.Fatalf("unexpected tool row %+v", block)
			}
			call := block.Calls[0]
			if call.Running != (index == active) {
				t.Fatalf("call %d running=%v, active=%d", index, call.Running, active)
			}
		}
	}
	for index, branch := range []string{"", "other"} {
		draft := protocol.ConversationDraft{Turn: turn, ID: "activity-" + branch, Branch: branch, Author: "sailry", Parts: []protocol.ConversationPart{{Kind: "resource", Data: json.RawMessage(`{"type":"tool_started","id":"shared","name":"read_file"}`)}}}
		apply("delta", draft)
		check(index)
		apply("hidden", protocol.ConversationChangeHiddenPayload{Turn: turn, ID: draft.ID})
		check(-1)
	}
}
