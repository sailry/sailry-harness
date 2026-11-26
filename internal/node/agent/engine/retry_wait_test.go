package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRetryWaitExhaustionPersistsWithoutExhaustingAttempts(t *testing.T) {
	t.Parallel()
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

func TestShared429CooldownAcrossConcurrentSessions(t *testing.T) {
	t.Parallel()
	var hits atomic.Int32
	m := &scriptedModel{scripts: []script{
		response(fantasy.StreamPart{
			Type: fantasy.StreamPartTypeError,
			Error: &fantasy.ProviderError{
				StatusCode: 429, Message: "too many requests",
				ResponseHeaders: map[string]string{"Retry-After": "45"},
			},
		}),
		response(text("Recovered A"), finish()),
		response(text("Completed B"), finish()),
	}}
	// Wrap scriptedModel to observe exact gateway call timing.
	wrapped := &scriptedModel{scripts: []script{
		func(ctx context.Context, call fantasy.Call, yield func(fantasy.StreamPart) bool) {
			hits.Add(1)
			m.scripts[0](ctx, call, yield)
		},
		func(ctx context.Context, call fantasy.Call, yield func(fantasy.StreamPart) bool) {
			hits.Add(1)
			m.scripts[1](ctx, call, yield)
		},
		func(ctx context.Context, call fantasy.Call, yield func(fantasy.StreamPart) bool) {
			hits.Add(1)
			m.scripts[2](ctx, call, yield)
		},
	}}
	f := newFixture(t, wrapped)
	// Create a second session on the same Runtime sharing the same provider/credential.
	config := f.session.Config
	var sessionB protocol.Session
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		req := protocol.NewRequest(f.node, protocol.Command{Kind: "create_session", Data: &protocol.CommandCreateSessionPayload{Config: &config}})
		out, _, err := store.ExecuteCommand(f.ctx, db, f.node, f.node, f.r.profile, req, store.PreparedPaths{})
		if err != nil {
			return err
		}
		sessionB = out.Data.(protocol.Session)
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	f.start()

	// Session A starts and hits HTTP 429 with Retry-After: 45.
	turnA := f.submit("Request A")
	clock := f.r.clock.(*manualClock)
	waitA := <-clock.waits
	if waitA.duration != 45*time.Second {
		t.Fatalf("session A wait = %s, want 45s", waitA.duration)
	}
	if hits.Load() != 1 {
		t.Fatalf("gateway hits during A's 429 = %d, want 1", hits.Load())
	}

	// While Session A is cooling down, Session B submits a turn on the same credential.
	inputB := protocol.DefaultConversationInput()
	inputB.Text = "Request B"
	submittedB := f.command("submit_turn", &protocol.CommandSubmitTurnPayload{
		Session: sessionB.ID, ExpectedRevision: sessionB.Revision, Message: inputB,
	})
	turnB := submittedB.Data.(protocol.QueuedTurn).ID
	waitB := <-clock.waits
	if waitB.duration <= 40*time.Second || waitB.duration > 45*time.Second {
		t.Fatalf("session B cooldown wait = %s, want ~45s", waitB.duration)
	}
	// Session B MUST NOT have reached the gateway while A's cooldown is active!
	if hits.Load() != 1 {
		t.Fatalf("session B hit the gateway %d times during A's 429 cooldown", hits.Load()-1)
	}

	// Release cooldown: both A and B complete with zero extra 429 hits.
	close(waitA.advance)
	close(waitB.advance)
	f.waitRun(turnA, protocol.ConversationStatusCompleted)
	f.waitRun(turnB, protocol.ConversationStatusCompleted)
	if hits.Load() != 3 {
		t.Fatalf("total gateway hits = %d, want 3 (A 429, A retry 200, B 200)", hits.Load())
	}
}
