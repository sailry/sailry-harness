package node

import (
	"context"
	"database/sql"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSessionRemovalStopsRunningSessionsFirst(t *testing.T) {
	t.Parallel()
	held := &shutdownScript{started: make(chan context.Context, 1), finish: make(chan struct{})}
	node := startShutdownNode(t, filepath.Join(t.TempDir(), "profile"), held)
	running := sessionFixture(t, node)
	idle := sessionFixture(t, node)
	ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
	defer cancel()
	turn := commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: running.ID, ExpectedRevision: running.Revision, Message: protocol.ConversationInput{Text: "Keep working"}}}).Data.(protocol.QueuedTurn)
	var stream context.Context
	select {
	case stream = <-held.started:
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	queued := commandFixture(t, node, protocol.Command{Kind: "queue_turn", Data: protocol.CommandQueueTurnPayload{Session: running.ID, ExpectedRevision: running.Revision, Message: protocol.ConversationInput{Text: "Never runs"}}}).Data.(protocol.QueuedTurn)

	output := commandFixture(t, node, protocol.Command{Kind: "remove_sessions", Data: protocol.CommandRemoveSessionsPayload{Expected: []protocol.SessionRevision{
		{Session: running.ID, Revision: running.Revision}, {Session: idle.ID, Revision: idle.Revision},
	}}})
	removed, ok := output.Data.([]protocol.SessionID)
	if output.Kind != "sessions_removed" || !ok || len(removed) != 2 || !slices.Contains(removed, running.ID) || !slices.Contains(removed, idle.ID) {
		t.Fatalf("removal returned %s %v", output.Kind, output.Data)
	}
	if stream.Err() == nil {
		t.Fatal("the running model request was not cancelled")
	}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		for _, id := range []protocol.TurnID{turn.ID, queued.ID} {
			run, err := store.ReadAgentRun(ctx, db, id)
			if err != nil {
				return err
			}
			if run.Status != protocol.ConversationStatusCancelled || run.FinishedMs == nil {
				t.Errorf("turn %s ended %s", id, run.Status)
			}
		}
		sessions, err := store.ReadSessions(ctx, db)
		if err != nil {
			return err
		}
		for _, session := range sessions {
			if session.ID == running.ID || session.ID == idle.ID {
				t.Errorf("session %s was not removed", session.ID)
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if calls := held.calls.Load(); calls != 1 {
		t.Fatalf("the removal admitted another model request: %d calls", calls)
	}
}

func TestStaleSessionRemovalStopsNothing(t *testing.T) {
	t.Parallel()
	held := &shutdownScript{started: make(chan context.Context, 1), finish: make(chan struct{})}
	node := startShutdownNode(t, filepath.Join(t.TempDir(), "profile"), held)
	running := sessionFixture(t, node)
	ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
	defer cancel()
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: running.ID, ExpectedRevision: running.Revision, Message: protocol.ConversationInput{Text: "Keep working"}}})
	var stream context.Context
	select {
	case stream = <-held.started:
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	result := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "remove_session", Data: protocol.CommandRemoveSessionPayload{Session: running.ID, ExpectedRevision: running.Revision + 1}}))
	if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("stale removal = %+v", result.Fault)
	}
	if stream.Err() != nil {
		t.Fatal("a stale removal stopped the running turn")
	}
	close(held.finish)
}
