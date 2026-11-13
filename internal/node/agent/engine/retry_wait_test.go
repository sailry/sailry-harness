package engine

import (
	"encoding/json"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRetryWaitExhaustionPersistsWithoutExhaustingAttempts(t *testing.T) {
	m := &scriptedModel{scripts: []script{response(fantasy.StreamPart{
		Type: fantasy.StreamPartTypeError,
		Error: &fantasy.ProviderError{
			StatusCode: 503, Message: "provider is temporarily busy",
			ResponseHeaders: map[string]string{"Retry-After": "601"},
		},
	})}}
	f := newFixture(t, m)
	f.start()
	turn := f.submit("Task")
	run := f.waitRun(turn, protocol.ConversationStatusFailed)
	if m.count() != 1 {
		t.Fatalf("an excessive retry wait sent another model request: %d", m.count())
	}
	select {
	case wait := <-f.r.clock.(*manualClock).waits:
		t.Fatalf("an excessive retry wait slept for %s", wait.duration)
	default:
	}
	if run.Error == nil || run.Error.Message != "provider is temporarily busy" || run.Error.Details == nil || run.Error.Details.Kind != "model_recovery" {
		t.Fatalf("the wait budget lost its provider cause: %+v", run.Error)
	}
	raw, err := json.Marshal(run.Error.Details.Data)
	if err != nil {
		t.Fatal(err)
	}
	var recovery protocol.FaultDetailsModelRecoveryPayload
	if err := json.Unmarshal(raw, &recovery); err != nil || recovery.Reason != protocol.ConversationRecoveryStopWaitExhausted {
		t.Fatalf("the stored turn has the wrong recovery reason: %+v, %v", recovery, err)
	}
	last := lastMessage(f.state().Records)
	if last == nil || last.Error == nil || last.Error.Code == nil || *last.Error.Code != string(model.Unavailable) || !last.Error.RetryWaitExhausted || last.Error.RetryExhausted {
		t.Fatalf("canonical reload lost the separate waiting budget: %+v", last)
	}
	// Round-trip the canonical codec as a later process does, independently of
	// the live Failure object which was returned by the model runner.
	raw, err = json.Marshal(last)
	if err != nil {
		t.Fatal(err)
	}
	var restored canonical.Message
	if err := json.Unmarshal(raw, &restored); err != nil || restored.Error == nil || !restored.Error.RetryWaitExhausted || restored.Error.RetryExhausted {
		t.Fatalf("the canonical codec changed the waiting budget: %+v, %v", restored.Error, err)
	}
}
