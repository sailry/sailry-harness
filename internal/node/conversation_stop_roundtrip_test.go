package node

import (
	"context"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConversationStopThenSendQueuedRoundTrip(t *testing.T) {
	held := &shutdownScript{started: make(chan context.Context, 2), finish: make(chan struct{})}
	node := startShutdownNode(t, filepath.Join(t.TempDir(), "profile"), held)
	session := sessionFixture(t, node)
	ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
	defer cancel()
	stream, err := node.subscribeConversation(ctx, session.ID)
	if err != nil {
		t.Fatal(err)
	}
	defer stream.Close()
	projection := client.NewConversationProjection(node.ID(), session.ID, 1)
	until := func(ready func(protocol.ConversationPage) bool) protocol.ConversationPage {
		t.Helper()
		for {
			update, err := stream.Next(ctx)
			if err != nil {
				t.Fatal(err)
			}
			if _, err := projection.Apply(1, update); err != nil {
				t.Fatalf("apply %s: %v", update.Kind, err)
			}
			page := projection.Snapshot().Page
			if ready(page) {
				return page
			}
		}
	}
	until(func(page protocol.ConversationPage) bool { return page.Session == session.ID })
	first := commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "First"}}}).Data.(protocol.QueuedTurn)
	select {
	case <-held.started:
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	next := commandFixture(t, node, protocol.Command{Kind: "queue_turn", Data: protocol.CommandQueueTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Send this after stopping"}}}).Data.(protocol.QueuedTurn)
	commandFixture(t, node, protocol.Command{Kind: "stop_turn", Data: protocol.CommandStopTurnPayload{Turn: first.ID}})
	paused := until(func(page protocol.ConversationPage) bool {
		return page.Queue.Paused && len(page.Queue.Items) == 1 && page.Queue.Items[0].Turn == next.ID
	})
	commandFixture(t, node, protocol.Command{Kind: "send_queued_turn", Data: protocol.CommandSendQueuedTurnPayload{Turn: next.ID, ExpectedRevision: paused.Queue.Items[0].Revision}})
	until(func(protocol.ConversationPage) bool {
		for _, draft := range projection.Snapshot().Drafts {
			if draft.Turn == next.ID && len(draft.Parts) > 0 && draft.Parts[0].Kind == "text" {
				return true
			}
		}
		return false
	})
	close(held.finish)
	until(func(page protocol.ConversationPage) bool {
		if len(page.Runs) != 2 || len(page.Queue.Items) != 0 || page.Queue.Paused {
			return false
		}
		return page.Runs[0].Turn == first.ID && page.Runs[0].Status == protocol.ConversationStatusCancelled && page.Runs[1].Turn == next.ID && page.Runs[1].Status == protocol.ConversationStatusCompleted
	})
	if calls := held.calls.Load(); calls != 2 {
		t.Fatalf("two admitted turns made %d model requests", calls)
	}
}
