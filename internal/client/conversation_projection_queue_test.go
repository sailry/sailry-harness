package client

import (
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func queueProjectionFixture() protocol.ConversationSnapshot {
	s := conversationFixture()
	item := protocol.DefaultConversationPending()
	item.Turn = protocol.TurnID{2}
	item.Request = new(protocol.RequestID{2})
	item.Kind = protocol.ConversationRunKindTask
	item.Revision, item.ConfigRevision = 2, 1
	item.Preview = "Pending input"
	s.Page.Queue.Revision = 5
	s.Page.Queue.Items = []protocol.ConversationPending{item}
	return s
}

func TestConversationQueueRevisionGuards(t *testing.T) {
	for _, test := range []struct {
		name   string
		change func(*protocol.ConversationQueue)
	}{
		{"older revision", func(q *protocol.ConversationQueue) { q.Revision-- }},
		{"unversioned pause", func(q *protocol.ConversationQueue) { q.Paused = true }},
		{"unversioned retained input", func(q *protocol.ConversationQueue) { q.Items = nil }},
		{"unversioned preview", func(q *protocol.ConversationQueue) { q.Revision++; q.Items[0].Preview = "Changed" }},
		{"unversioned request", func(q *protocol.ConversationQueue) { q.Revision++; q.Items[0].Request = new(protocol.RequestID{3}) }},
		{"older item revision", func(q *protocol.ConversationQueue) { q.Revision++; q.Items[0].Revision-- }},
		{"changed capture", func(q *protocol.ConversationQueue) { q.Revision++; q.Items[0].ConfigRevision++ }},
		{"invalid item revision", func(q *protocol.ConversationQueue) { q.Revision++; q.Items[0].Revision = 0 }},
	} {
		t.Run(test.name, func(t *testing.T) {
			s := queueProjectionFixture()
			p := NewConversationProjection(s.Node, s.Page.Session, 1)
			if got, err := p.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: s}); got != Applied || err != nil {
				t.Fatalf("snapshot: %v, %v", got, err)
			}
			before, transcript := p.Snapshot(), p.transcript
			queue := copyValue(s.Page.Queue)
			test.change(&queue)
			if got, err := p.Apply(1, conversationFrame(s, "queue", queue)); got != Ignored || faultCode(err) != protocol.ErrorCodeInvalidRequest {
				t.Fatalf("invalid queue was not rejected: %v, %v", got, err)
			}
			if !reflect.DeepEqual(before, p.Snapshot()) || transcript != p.transcript {
				t.Fatal("invalid queue changed the snapshot, sequence or transcript version")
			}
		})
	}
}

func TestConversationQueueReplayAndRecovery(t *testing.T) {
	s := queueProjectionFixture()
	p := NewConversationProjection(s.Node, s.Page.Session, 1)
	if got, err := p.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: s}); got != Applied || err != nil {
		t.Fatalf("snapshot: %v, %v", got, err)
	}
	transcript := p.transcript
	duplicate := conversationFrame(s, "queue", copyValue(s.Page.Queue))
	if got, err := p.Apply(1, duplicate); got != Applied || err != nil || p.transcript != transcript || p.snapshot.Sequence != s.Sequence+1 {
		t.Fatalf("identical queue did not advance only the feed sequence: %v, %v", got, err)
	}
	before := p.Snapshot()
	if got, err := p.Apply(1, duplicate); got != Ignored || err != nil || !reflect.DeepEqual(before, p.Snapshot()) {
		t.Fatalf("repeated frame changed the projection: %v, %v", got, err)
	}

	queue := copyValue(before.Page.Queue)
	queue.Revision += 3
	queue.Items[0].Revision++
	queue.Items[0].Preview = "Edited input"
	queue.Items[0].Request = new(protocol.RequestID{3})
	if got, err := p.Apply(1, conversationFrame(*before, "queue", queue)); got != Applied || err != nil || p.transcript != transcript+1 {
		t.Fatalf("forward queue revision jump was rejected: %v, %v", got, err)
	}
	before = p.Snapshot()
	queue.Revision++
	queue.Items[0].Ready = true
	missing := conversationFrame(*before, "queue", copyValue(queue))
	queue.Revision++
	queue.Items = nil
	later := conversationFrame(*before, "queue", queue)
	frame := later.Data.(protocol.ConversationFrame)
	frame.Sequence++
	later.Data = frame
	if got, err := p.Apply(1, later); got != Recover || err != nil || !reflect.DeepEqual(before, p.Snapshot()) {
		t.Fatalf("feed gap did not retain the last coherent queue: %v, %v", got, err)
	}
	if got, err := p.Apply(1, missing); got != Recover || err != nil || !reflect.DeepEqual(before, p.Snapshot()) {
		t.Fatalf("missing frame bypassed snapshot recovery: %v, %v", got, err)
	}

	recovered := copyValue(*before)
	recovered.Sequence = frame.Sequence
	recovered.Page.Queue = copyValue(queue)
	if got, err := p.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: recovered}); got != Applied || err != nil {
		t.Fatalf("recovery snapshot: %v, %v", got, err)
	}
	for _, replay := range []protocol.Update{duplicate, missing, later} {
		if got, err := p.Apply(1, replay); got != Ignored || err != nil || !reflect.DeepEqual(&recovered, p.Snapshot()) {
			t.Fatalf("old replay replaced recovered state: %v, %v", got, err)
		}
	}
	if err := p.Reconnect(2); err != nil {
		t.Fatal(err)
	}
	if got, err := p.Apply(1, conversationFrame(recovered, "queue", s.Page.Queue)); got != Ignored || err != nil {
		t.Fatalf("old generation affected recovery: %v, %v", got, err)
	}
	if got, err := p.Apply(2, protocol.Update{Kind: "conversation_snapshot", Data: recovered}); got != Applied || err != nil {
		t.Fatalf("new generation snapshot: %v, %v", got, err)
	}
}
