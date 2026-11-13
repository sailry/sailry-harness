package live

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRetryWaitBudgetProjectsItsOwnTerminalReason(t *testing.T) {
	for _, hadRetry := range []bool{false, true} {
		t.Run(map[bool]string{false: "before_first_retry", true: "after_retry"}[hadRetry], func(t *testing.T) {
			code := string(model.Unavailable)
			encoded, err := json.Marshal(canonical.ResponseError{Code: &code, Message: "provider busy", RetryWaitExhausted: true})
			if err != nil {
				t.Fatal(err)
			}
			var saved canonical.ResponseError
			if err := json.Unmarshal(encoded, &saved); err != nil {
				t.Fatal(err)
			}
			fault := model.Fault(&model.Failure{Kind: model.FailureKind(*saved.Code), Message: saved.Message,
				Exhausted: saved.RetryExhausted, WaitExhausted: saved.RetryWaitExhausted})
			snapshot := protocol.DefaultConversationSnapshot()
			snapshot.Node, snapshot.Sequence = protocol.NodeID{1}, 1
			snapshot.Page.Session, snapshot.Page.Revision = protocol.SessionID{2}, 1
			run := protocol.DefaultConversationRun()
			run.Worktree, run.Session, run.Turn = protocol.WorktreeID{3}, snapshot.Page.Session, protocol.TurnID{4}
			run.Sequence, run.Revision, run.Kind = 1, 1, protocol.ConversationRunKindTask
			run.Status, run.Error = protocol.ConversationStatusFailed, fault
			snapshot.Page.Runs = []protocol.ConversationRun{run}
			if hadRetry {
				entry := protocol.DefaultConversationEntry()
				entry.ID, entry.Sequence, entry.Turn, entry.Author = "retry", 1, run.Turn, "assistant"
				entry.Parts = []protocol.ConversationPart{{Kind: "resource", Data: json.RawMessage(`{"type":"model_retry","attempt":2,"limit":9,"delay_ms":1000,"error":"provider busy"}`)}}
				snapshot.Page.Entries = []protocol.ConversationEntry{entry}
			}
			// Exercise the public wire decoder before the projection, as a
			// reconnecting controller receiving this stored turn does.
			encoded, err = json.Marshal(snapshot)
			if err != nil {
				t.Fatal(err)
			}
			var restored protocol.ConversationSnapshot
			if err := protocol.DecodeAs(protocol.PeerData, encoded, &restored); err != nil {
				t.Fatal(err)
			}
			turns, session, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: restored}, "en")
			if len(turns) != 1 || turns[0].Reply.Status != conversation.Failed || session.Diagnostic == nil || session.Diagnostic.Title != "chat_model_retry_wait_exhausted" {
				t.Fatalf("waiting budget lost its terminal presentation: turns=%+v diagnostic=%+v", turns, session.Diagnostic)
			}
			var retry *conversation.Retry
			var title string
			for _, block := range turns[0].Reply.Blocks {
				if block.Retry != nil {
					retry = block.Retry
				}
				if block.Kind == conversation.ErrorBlock {
					title = block.Label
				}
			}
			if title != "chat_model_retry_wait_exhausted" {
				t.Fatalf("waiting budget was shown as another failure: %q", title)
			}
			if !hadRetry {
				if retry != nil {
					t.Fatal("a wait refused before the first retry invented a retry row")
				}
				return
			}
			if retry == nil || retry.Exhausted || !retry.Stopped || retry.Reason != title || retry.Attempt != 2 || retry.Limit != 9 {
				t.Fatalf("waiting budget changed the attempts or lost its reason: %+v", retry)
			}
		})
	}
}
