package engine

import (
	"context"
	"database/sql"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/feed"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestResponsesObservationHandoffAndRestart(t *testing.T) {
	response, err := os.ReadFile("../model/testdata/responses-text.sse")
	if err != nil {
		t.Fatal(err)
	}
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Method != http.MethodPost || request.URL.Path != "/v1/responses" {
			writer.WriteHeader(http.StatusNotFound)
			return
		}
		if _, err := io.Copy(io.Discard, request.Body); err != nil {
			t.Error(err)
			return
		}
		requests.Add(1)
		writer.Header().Set("Content-Type", "text/event-stream")
		_, _ = writer.Write(response)
	}))
	t.Cleanup(server.Close)
	f := newFixture(t, nil)
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(f.ctx, `UPDATE providers SET body=json_set(body,'$.provider.api','responses','$.provider.endpoint',?1)`, server.URL+"/v1")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	f.r.model = model.New
	finished := make(chan struct{})
	gate, release := context.WithCancel(f.ctx)
	defer release()
	var turns atomic.Int32
	f.r.afterTurn = func(context.Context, *store.AgentInvocation, protocol.ConversationRun, TurnObservation) {
		if turns.Add(1) == 1 {
			close(finished)
			<-gate.Done()
		}
	}
	readObservation := func(turn protocol.TurnID) *protocol.ConversationObservationRequest {
		t.Helper()
		var observation *protocol.ConversationObservationRequest
		for _, record := range f.state().Records {
			if execution := record.Metadata.Execution; record.Turn == turn && execution != nil && execution.Observation != nil {
				if observation != nil {
					t.Fatal("expected exactly one persisted request for the turn")
				}
				observation = execution.Observation
			}
		}
		if observation == nil || len(observation.Attempts) != 1 {
			t.Fatal("missing persisted attempt")
		}
		return observation
	}

	f.start()
	first := f.submit("Initial request")
	select {
	case <-finished:
	case <-f.ctx.Done():
		t.Fatal("first execution did not settle")
	}
	firstObservation := readObservation(first)
	if firstObservation.Attempts[0].ResponsesBoundary != nil {
		t.Fatal("first attempt has no predecessor")
	}
	// Start and finish the next turn while the first worker is still unwinding.
	second := f.submit("Follow-up request")
	f.waitRun(second, protocol.ConversationStatusCompleted)
	secondObservation := readObservation(second)
	boundary := secondObservation.Attempts[0].ResponsesBoundary
	if boundary == nil || boundary.PreviousTurn != first || boundary.PreviousRequestID != firstObservation.ID || boundary.PreviousAttempt != 1 || boundary.PreviousCachedInput == nil || *boundary.PreviousCachedInput != 8 {
		t.Fatalf("persisted cross-turn boundary = %+v", boundary)
	}
	var userAdded, assistantAdded bool
	for _, change := range boundary.InputChanges {
		if change.Added.Items > 0 && change.Added.Bytes > 0 {
			userAdded = userAdded || change.Kind == protocol.ConversationResponsesInputUser
			assistantAdded = assistantAdded || change.Kind == protocol.ConversationResponsesInputAssistant
		}
	}
	if !userAdded || !assistantAdded {
		t.Fatalf("follow-up omitted the added user and assistant item sizes: %+v", boundary.InputChanges)
	}

	prior := f.r
	release()
	prior.Stop()
	f.r = New(f.db.Node, f.node, prior.profile, feed.New(f.node), prior.publish, Options{Clock: prior.clock, Model: model.New})
	if err := f.r.Recover(f.ctx); err != nil {
		t.Fatal(err)
	}
	f.start()
	third := f.submit("Request after restart")
	f.waitRun(third, protocol.ConversationStatusCompleted)
	if readObservation(third).Attempts[0].ResponsesBoundary != nil || requests.Load() != 3 {
		t.Fatal("runtime restart invented a predecessor or replayed a request")
	}
	if got := readObservation(second).Attempts[0].ResponsesBoundary; got == nil || got.PreviousRequestID != firstObservation.ID {
		t.Fatal("runtime restart lost the persisted comparison")
	}
}
