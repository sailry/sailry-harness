package client

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Only a sessions_removed event reports a removal: a snapshot that stops
// listing a session, as after a permission is withdrawn, reports none, and
// each reader continues from where it read.
func TestRemovedSessionsOnlyFromEvents(t *testing.T) {
	node := protocol.NodeID{1}
	c := &Client{project: NewProjection(node, 1)}
	snapshot := func(cursor uint64, ids ...protocol.SessionID) {
		t.Helper()
		value := protocol.DefaultSnapshot()
		value.Node, value.Cursor = node, cursor
		for _, id := range ids {
			session := protocol.DefaultSession()
			session.ID = id
			value.Sessions = append(value.Sessions, session)
		}
		if applied, err := c.project.Apply(1, protocol.Update{Kind: "snapshot", Data: value}); applied != Applied || err != nil {
			t.Fatalf("snapshot at %d: %v %v", cursor, applied, err)
		}
	}
	a, b := protocol.SessionID{1}, protocol.SessionID{2}
	snapshot(1, a, b)
	snapshot(2, a)
	if removed, next := c.RemovedSessions(0); len(removed) != 0 || next != 0 {
		t.Fatalf("a session absent from a snapshot was reported removed: %v", removed)
	}
	snapshot(3, a, b)
	event := protocol.EventEnvelope{Node: node, Cursor: 4, Event: protocol.Event{Kind: "sessions_removed", Data: []protocol.SessionID{b}}}
	if applied, err := c.project.Apply(1, protocol.Update{Kind: "event", Data: event}); applied != Applied || err != nil {
		t.Fatalf("event: %v %v", applied, err)
	}
	removed, next := c.RemovedSessions(0)
	if !slices.Equal(removed, []protocol.SessionID{b}) || next != 1 {
		t.Fatalf("removed %v, next %d", removed, next)
	}
	if again, at := c.RemovedSessions(next); len(again) != 0 || at != next {
		t.Fatalf("a read removal was reported again: %v", again)
	}
	// The log keeps the latest removedLimit; a reader further behind gets
	// those alone and keeps the older ones it missed.
	many := make([]protocol.SessionID, removedLimit+10)
	for i := range many {
		many[i] = protocol.SessionID{byte(i), byte(i >> 8), 9}
	}
	event = protocol.EventEnvelope{Node: node, Cursor: 5, Event: protocol.Event{Kind: "sessions_removed", Data: many}}
	if applied, err := c.project.Apply(1, protocol.Update{Kind: "event", Data: event}); applied != Applied || err != nil {
		t.Fatalf("bulk event: %v %v", applied, err)
	}
	if len(c.project.removed) != removedLimit {
		t.Fatalf("the log holds %d removals, want %d", len(c.project.removed), removedLimit)
	}
	removed, at := c.RemovedSessions(next)
	if at != uint64(len(many))+1 || !slices.Equal(removed, many[len(many)-removedLimit:]) {
		t.Fatalf("behind the log: %d removals, next %d", len(removed), at)
	}
	if removed, _ := c.RemovedSessions(at - 2); !slices.Equal(removed, many[len(many)-2:]) {
		t.Fatalf("within the log: %v", removed)
	}
}
