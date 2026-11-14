package client

import (
	"context"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConversationFailureStopsLoadingAndCanRefresh(t *testing.T) {
	snapshot := projectionSnapshot(0)
	session := projectionSession(1, 1)
	snapshot.Sessions = []protocol.Session{session}
	snapshot.Worktrees = []protocol.Worktree{{ID: session.Worktree}}
	transport := &conversationTransport{node: testSubscription(snapshot), opened: make(chan *clientSubscription, 8)}
	controller := New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	changes := make(chan struct{}, 8)
	release, err := controller.WatchConversation(t.Context(), session.ID, func() {
		select {
		case changes <- struct{}{}:
		default:
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	wait := func(ready func(ConversationState) bool) ConversationState {
		t.Helper()
		ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
		defer cancel()
		for {
			state, _ := controller.Conversation(session.ID)
			if ready(state) {
				return state
			}
			select {
			case <-changes:
			case <-ctx.Done():
				t.Fatal("conversation did not reach expected state", state, ctx.Err())
			}
		}
	}
	first := <-transport.opened
	state := wait(func(state ConversationState) bool { return state.Connected })
	run := state.Snapshot.Page.Runs[0]
	run.Session = protocol.SessionID{2}
	first.updates <- conversationFrame(state.Snapshot, "run", run)
	failed := wait(func(state ConversationState) bool { return state.Err != nil })
	if failed.Loading || failed.Connected || faultCode(failed.Err) != protocol.ErrorCodeWrongTarget || failed.Snapshot.Sequence != state.Snapshot.Sequence {
		t.Fatalf("rejected update left loading or changed accepted history: %+v", failed)
	}
	if !controller.Connected() {
		t.Fatal("conversation failure disconnected the Host")
	}
	if err := controller.RefreshConversation(t.Context(), session.ID); err != nil {
		t.Fatal(err)
	}
	<-transport.opened
	resumed := wait(func(state ConversationState) bool {
		return state.Connected && state.Snapshot.Sequence > failed.Snapshot.Sequence
	})
	if resumed.Loading || resumed.Err != nil {
		t.Fatalf("successful refresh kept the old failure: %+v", resumed)
	}
}

func TestHistoryReadStopsWaitingForRejectedConversationSequence(t *testing.T) {
	snapshot := conversationFixture()
	snapshot.Page.NextBefore = &snapshot.Page.Runs[0].Turn
	transport := &clientTransport{dispatch: func(request protocol.Request) (link.Admission, error) {
		page := protocol.DefaultConversationPage()
		page.Session, page.Revision = snapshot.Page.Session, snapshot.Page.Revision
		completion := make(chan link.Response, 1)
		completion <- link.Response{Output: protocol.Output{Kind: "conversation", Data: protocol.ConversationHistory{Sequence: snapshot.Sequence + 1, Page: page}}}
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
	}}
	controller := New(transport)
	defer controller.Close()
	projection := NewConversationProjection(snapshot.Node, snapshot.Page.Session, 1)
	projection.snapshot = &snapshot
	watch := &conversationWatch{session: snapshot.Page.Session, ctx: t.Context(), projection: projection, signal: make(chan struct{}), state: ConversationState{Err: fault(protocol.ErrorCodeWrongTarget, "run belongs to another conversation")}}
	controller.conversations = map[protocol.SessionID]*conversationWatch{snapshot.Page.Session: watch}
	defer func() { controller.conversations = nil }()
	ctx, cancel := context.WithTimeout(t.Context(), time.Second)
	defer cancel()
	err := controller.LoadOlderConversation(ctx, snapshot.Page.Session)
	if faultCode(err) != protocol.ErrorCodeWrongTarget || watch.state.LoadingOlder || faultCode(watch.state.OlderErr) != protocol.ErrorCodeWrongTarget {
		t.Fatalf("history waited after observation stopped: error=%v, state=%+v", err, watch.state)
	}
	if projection.snapshot.Page.NextBefore == nil {
		t.Fatal("failed history read changed pagination")
	}
}
