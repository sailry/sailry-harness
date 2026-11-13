package live

import (
	"context"
	"encoding/json"
	"fmt"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type projectionRetryModel struct {
	fantasy.LanguageModel
	calls   int
	partial bool
}

func (m *projectionRetryModel) Stream(context.Context, fantasy.Call) (fantasy.StreamResponse, error) {
	m.calls++
	last := m.calls == model.MainRetries+1
	return func(yield func(fantasy.StreamPart) bool) {
		if last && m.partial && !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Visible partial reply"}) {
			return
		}
		yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.Network, Message: "Fixture connection failed"}})
	}, nil
}

type projectionRetryClock struct {
	now   time.Time
	waits []time.Duration
}

func (c *projectionRetryClock) Now() time.Time { return c.now }
func (c *projectionRetryClock) Sleep(ctx context.Context, delay time.Duration) error {
	c.waits = append(c.waits, delay)
	c.now = c.now.Add(delay)
	return ctx.Err()
}

func TestModelRetryOutcomeSurvivesCanonicalReloadAndProjection(t *testing.T) {
	for _, partial := range []bool{false, true} {
		t.Run(fmt.Sprintf("partial_%t", partial), func(t *testing.T) {
			wantExhausted := !partial
			session, turn := protocol.SessionID{2}, protocol.TurnID{3}
			snapshot := protocol.DefaultConversationSnapshot()
			snapshot.Node, snapshot.Sequence = protocol.NodeID{1}, 1
			snapshot.Page.Session, snapshot.Page.Revision = session, 1
			appendEntry := func(part protocol.ConversationPart) {
				entry := protocol.DefaultConversationEntry()
				entry.ID = fmt.Sprintf("fixture-%d", len(snapshot.Page.Entries))
				entry.Sequence, entry.Turn, entry.Author = uint64(len(snapshot.Page.Entries)+1), turn, "assistant"
				entry.Parts = []protocol.ConversationPart{part}
				snapshot.Page.Entries = append(snapshot.Page.Entries, entry)
			}
			language := &projectionRetryModel{partial: partial}
			clock := &projectionRetryClock{now: time.Date(2026, 11, 10, 0, 0, 0, 0, time.UTC)}
			err := model.Run(t.Context(), language, fantasy.Call{}, model.Main, clock, func(attempt model.Attempt) error {
				if attempt.RetryDelay != nil {
					raw, err := json.Marshal(map[string]any{"type": "model_retry", "attempt": attempt.Index + 1, "limit": model.MainRetries,
						"delay_ms": attempt.RetryDelay.Milliseconds(), "error": attempt.Failure.Message})
					if err != nil {
						return err
					}
					appendEntry(protocol.ConversationPart{Kind: "resource", Data: json.RawMessage(raw)})
				}
				return nil
			}, func(part fantasy.StreamPart) bool {
				if part.Type == fantasy.StreamPartTypeTextDelta {
					appendEntry(protocol.ConversationPart{Kind: "text", Data: part.Delta})
				}
				return true
			})
			if err == nil || language.calls != model.MainRetries+1 || len(clock.waits) != model.MainRetries {
				t.Fatalf("unexpected fixture recovery: calls=%d waits=%v error=%v", language.calls, clock.waits, err)
			}
			// Follow infer -> canonical JSON -> turn's reconstruction, rather
			// than supplying the UI a hand-authored model_recovery fault.
			failure := model.Classify(err)
			code := string(failure.Kind)
			raw, err := json.Marshal(canonical.ResponseError{Code: &code, Message: failure.Message, RetryExhausted: failure.Exhausted})
			if err != nil {
				t.Fatal(err)
			}
			var saved canonical.ResponseError
			if err := json.Unmarshal(raw, &saved); err != nil {
				t.Fatal(err)
			}
			if saved.Code == nil || *saved.Code != string(model.Network) || saved.RetryExhausted != wantExhausted {
				t.Fatalf("canonical error lost its cause or recovery state: %+v", saved)
			}
			fault := model.Fault(&model.Failure{Kind: model.FailureKind(*saved.Code), Message: saved.Message, Exhausted: saved.RetryExhausted})
			run := protocol.DefaultConversationRun()
			run.Worktree, run.Session, run.Turn = protocol.WorktreeID{4}, session, turn
			run.Sequence, run.Revision = 1, 1
			run.Kind, run.Status, run.Error = protocol.ConversationRunKindTask, protocol.ConversationStatusFailed, fault
			snapshot.Page.Runs = []protocol.ConversationRun{run}
			turns, _, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: snapshot}, "en")
			if len(turns) != 1 || turns[0].Reply.Status != conversation.Failed {
				t.Fatalf("failed retry did not project its turn: %+v", turns)
			}
			var retry *conversation.Retry
			var text, title string
			for _, block := range turns[0].Reply.Blocks {
				if block.Retry != nil {
					retry = block.Retry
				}
				if block.Kind == conversation.TextBlock {
					text += block.Text
				}
				if block.Kind == conversation.ErrorBlock {
					title = block.Label
				}
			}
			if retry == nil || retry.Attempt != model.MainRetries || retry.Limit != model.MainRetries || retry.Exhausted != wantExhausted || retry.Stopped != wantExhausted || retry.Detail != "Fixture connection failed" {
				t.Fatalf("retry projection lost the actual recovery outcome: %+v", retry)
			}
			if partial {
				if text != "Visible partial reply" || title == "chat_model_retry_exhausted" {
					t.Fatalf("partial output was lost or mislabeled exhausted: text=%q title=%q", text, title)
				}
			} else if title != "chat_model_retry_exhausted" || retry.Reason != title {
				t.Fatalf("exhaustion was not shown as the terminal reason: title=%q retry=%+v", title, retry)
			}
		})
	}
}
