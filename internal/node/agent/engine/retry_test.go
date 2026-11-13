package engine

import (
	"encoding/json"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRetryExhaustionKeepsProviderCause(t *testing.T) {
	for _, summary := range []bool{false, true} {
		name := "main"
		if summary {
			name = "summary"
		}
		t.Run(name, func(t *testing.T) {
			m := &scriptedModel{}
			if summary {
				m.scripts = append(m.scripts, response(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.ContextOverflow, Message: "context full"}}))
			}
			for range model.MainRetries + 1 {
				m.scripts = append(m.scripts, response(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.RateLimited, Message: "provider rate limited"}}))
			}
			f := newFixture(t, m)
			f.start()
			turn := f.submit("Task")
			for range model.MainRetries {
				select {
				case wait := <-f.r.clock.(*manualClock).waits:
					if wait.duration != 60*time.Second {
						t.Fatalf("retry wait = %s", wait.duration)
					}
					close(wait.advance)
				case <-f.ctx.Done():
					t.Fatal("the transient request did not reach its retry budget")
				}
			}
			run := f.waitRun(turn, protocol.ConversationStatusFailed)
			if run.Error == nil || run.Error.Message != "provider rate limited" || run.Error.Details == nil || run.Error.Details.Kind != "model_recovery" {
				t.Fatalf("the exhausted turn lost its cause or recovery outcome: %+v", run.Error)
			}
			raw, err := json.Marshal(run.Error.Details.Data)
			if err != nil {
				t.Fatal(err)
			}
			var recovery protocol.FaultDetailsModelRecoveryPayload
			if err := json.Unmarshal(raw, &recovery); err != nil || recovery.Reason != protocol.ConversationRecoveryStopAttemptsExhausted {
				t.Fatalf("invalid exhausted recovery: %+v, %v", recovery, err)
			}
			wantCalls := model.MainRetries + 1
			if summary {
				wantCalls++ // The initial main request requested compaction.
			}
			if m.count() != wantCalls {
				t.Fatalf("exhaustion sent %d requests, want %d", m.count(), wantCalls)
			}
			state := f.state()
			if !summary {
				last := lastMessage(state.Records)
				if last == nil || last.Error == nil || last.Error.Code == nil || *last.Error.Code != string(model.RateLimited) || last.Error.Message != "provider rate limited" || !last.Error.RetryExhausted {
					t.Fatalf("the canonical error lost the provider classification: %+v", last)
				}
				return
			}
			for _, record := range state.Records {
				if maintenance := record.Metadata.Compaction; maintenance != nil && maintenance.Failure != nil {
					failure := maintenance.Failure
					if failure.Reason != protocol.ConversationCompactionReasonRateLimited || failure.Cause == nil || *failure.Cause != "provider rate limited" {
						t.Fatalf("summary exhaustion lost the provider diagnosis: %+v", failure)
					}
					return
				}
			}
			t.Fatal("summary exhaustion has no compaction failure record")
		})
	}
}
