package client

import (
	"context"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type conversationTransport struct {
	mu       sync.Mutex
	requests []protocol.Request
	opened   chan *clientSubscription
	node     *clientSubscription
	sequence uint64
	count    int
}

func (t *conversationTransport) Target() protocol.NodeID { return protocol.NodeID{1} }
func (t *conversationTransport) Subscribe(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "node" {
		return t.node, nil
	}
	t.mu.Lock()
	t.count++
	t.sequence = uint64(8 + 2*t.count)
	sequence := t.sequence
	t.mu.Unlock()
	s := conversationFixture()
	s.Sequence = sequence
	s.Page.Runs[0].Turn = protocol.TurnID{3}
	s.Page.Runs[0].Sequence = 3
	s.Page.Runs[0].Status = "completed"
	s.Page.Entries[0].Turn = protocol.TurnID{3}
	s.Page.Entries[0].Sequence = 3
	s.Page.NextBefore = new(protocol.TurnID{3})
	sub := &clientSubscription{updates: make(chan protocol.Update, 8), closed: make(chan struct{})}
	sub.updates <- protocol.Update{Kind: "conversation_snapshot", Data: s}
	t.opened <- sub
	return sub, nil
}
func (t *conversationTransport) Dispatch(_ context.Context, request protocol.Request) (link.Admission, error) {
	t.mu.Lock()
	t.requests = append(t.requests, request)
	sequence := t.sequence
	t.mu.Unlock()
	p := protocol.DefaultConversationPage()
	p.Session = protocol.SessionID{1}
	p.Revision = 1
	for i := byte(1); i <= 2; i++ {
		p.Runs = append(p.Runs, protocol.ConversationRun{Session: p.Session, Turn: protocol.TurnID{i}, Sequence: uint64(i), Revision: 1, Kind: "task", Status: "completed"})
		p.Entries = append(p.Entries, protocol.ConversationEntry{ID: string([]byte{'a' + i}), Turn: protocol.TurnID{i}, Sequence: uint64(i), Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Older"}}})
	}
	done := make(chan link.Response, 1)
	done <- link.Response{Output: protocol.Output{Kind: "conversation", Data: protocol.ConversationHistory{Sequence: sequence, Page: p}}}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: done}, nil
}

func testConversationWatchRecovery(t *testing.T) {
	snapshot := projectionSnapshot(0)
	session := projectionSession(1, 1)
	snapshot.Sessions = []protocol.Session{session}
	snapshot.Worktrees = []protocol.Worktree{{ID: session.Worktree}}
	transport := &conversationTransport{node: testSubscription(snapshot), opened: make(chan *clientSubscription, 8)}
	c := New(transport)
	if err := c.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer c.Close()
	changes := make(chan struct{}, 32)
	changed := func() {
		select {
		case changes <- struct{}{}:
		default:
		}
	}
	wait := func(predicate func(ConversationState) bool) {
		t.Helper()
		for {
			state, ok := c.Conversation(session.ID)
			if ok && predicate(state) {
				return
			}
			select {
			case <-changes:
			case <-t.Context().Done():
				t.Fatal(t.Context().Err())
			}
		}
	}
	release, err := c.WatchConversation(t.Context(), session.ID, changed)
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	other, err := c.WatchConversation(t.Context(), session.ID, changed)
	if err != nil {
		t.Fatal(err)
	}
	defer other()
	first := <-transport.opened
	wait(func(s ConversationState) bool { return s.Connected })
	transport.mu.Lock()
	count := transport.count
	transport.mu.Unlock()
	if count != 1 {
		t.Fatal("observers did not share a subscription")
	}
	if err := c.LoadOlderConversation(t.Context(), session.ID); err != nil {
		t.Fatal(err)
	}
	wait(func(s ConversationState) bool {
		return len(s.Snapshot.Page.Runs) == 3 && s.Snapshot.Page.NextBefore == nil
	})
	if err := c.RefreshConversation(t.Context(), session.ID); err != nil {
		t.Fatal(err)
	}
	second := <-transport.opened
	wait(func(s ConversationState) bool {
		return s.Connected && s.Snapshot.Sequence == 12 && len(s.Snapshot.Page.Runs) == 3 && s.Snapshot.Page.NextBefore == nil
	})
	select {
	case <-first.closed:
	default:
		t.Fatal("refresh retained its old stream")
	}
	transport.mu.Lock()
	requests := append([]protocol.Request(nil), transport.requests...)
	transport.mu.Unlock()
	if len(requests) != 2 {
		t.Fatalf("history reads=%d", len(requests))
	}
	for _, request := range requests {
		query, ok := request.Command.Data.(protocol.CommandReadConversationPayload)
		if request.Command.Kind != "read_conversation" || !ok || query.Limit != 20 || query.Before == nil || *query.Before != (protocol.TurnID{3}) {
			t.Fatal("reconnect replayed business input or lost pagination scope")
		}
	}
	release()
	select {
	case <-second.closed:
		t.Fatal("releasing one observer stopped another")
	default:
	}
	transport.node.updates <- projectionEvent(1, "sessions_removed", []protocol.SessionID{session.ID})
	wait(func(s ConversationState) bool { return s.Removed })
	select {
	case <-second.closed:
	case <-t.Context().Done():
		t.Fatal(t.Context().Err())
	}
	other()
	if err := c.Close(); err != nil {
		t.Fatal(err)
	}
}

func (t *conversationTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "fixture stream is unavailable"}
}
