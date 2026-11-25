package engine

import (
	"context"
	"database/sql"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAfterTurnKeepsSessionOrder(t *testing.T) {
	t.Parallel()
	for _, row := range []struct {
		name     string
		panics   bool
		shutdown bool
	}{
		{name: "settlement releases before observers"},
		{name: "panicking settlement releases", panics: true},
		{name: "stopping releases without starting a successor", shutdown: true},
	} {
		t.Run(row.name, func(t *testing.T) {
			t.Parallel()
			f := newFixture(t, &scriptedModel{scripts: []script{
				response(text("First"), finish()), response(text("Other session"), finish()), response(text("Successor"), finish()),
			}})
			other := f.command("create_session_at", &protocol.CommandCreateSessionAtPayload{Target: f.node, Config: &f.session.Config}).Data.(protocol.Session)
			first := f.submit("First request")
			entered, release := make(chan struct{}), make(chan struct{})
			observing, observed := make(chan struct{}), make(chan struct{})
			f.r.turnCompletion = func(_ context.Context, inv *store.AgentInvocation) TurnCompletion {
				if inv.Run.Turn != first {
					return TurnCompletion{}
				}
				return TurnCompletion{
					Settle: func(ctx context.Context, _ protocol.ConversationRun, _ TurnObservation) {
						close(entered)
						select {
						case <-release:
							if row.panics {
								panic("fixture settlement panic")
							}
						case <-ctx.Done():
						}
					},
					Observe: func(ctx context.Context, _ protocol.ConversationRun, _ TurnObservation) {
						close(observing)
						select {
						case <-observed:
						case <-ctx.Done():
						}
					},
				}
			}
			f.start()
			select {
			case <-entered:
			case <-f.ctx.Done():
				t.Fatal("first turn did not enter ordered settlement")
			}
			f.waitRun(first, protocol.ConversationStatusCompleted)
			second := f.submit("Queued guidance")
			input := protocol.DefaultConversationInput()
			input.Text = "Unrelated request"
			unrelated := f.command("submit_turn", &protocol.CommandSubmitTurnPayload{Session: other.ID, ExpectedRevision: other.Revision, Message: input}).Data.(protocol.QueuedTurn)
			f.waitRun(unrelated.ID, protocol.ConversationStatusCompleted)
			if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
				run, err := store.ReadAgentRun(f.ctx, db, second)
				if err == nil && run.Status != protocol.ConversationStatusQueued {
					t.Errorf("successor ran before the first turn's settlement completed: %s", run.Status)
				}
				return err
			}); err != nil {
				t.Fatal(err)
			}
			if row.shutdown {
				f.r.Stop()
				if len(f.r.afterTurnSessions) != 0 {
					t.Fatal("a cancelled settlement retained the session after shutdown")
				}
				select {
				case <-observing:
					t.Fatal("shutdown started the ordinary observer")
				default:
				}
				f.waitRun(second, protocol.ConversationStatusQueued)
				return
			}
			close(release)
			if !row.panics {
				select {
				case <-observing:
				case <-f.ctx.Done():
					t.Fatal("first turn did not enter its ordinary observer")
				}
			}
			f.waitRun(second, protocol.ConversationStatusCompleted)
			close(observed)
		})
	}
}
