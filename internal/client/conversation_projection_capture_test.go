package client

import (
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func guidanceCaptureFixture() protocol.ConversationSnapshot {
	s := queueProjectionFixture()
	item := &s.Page.Queue.Items[0]
	item.ConfigRevision = 3
	item.Guidance = new(s.Page.Runs[0].Turn)
	run := protocol.DefaultConversationRun()
	run.Session, run.Turn = s.Page.Session, item.Turn
	run.Sequence, run.Revision = 2, item.ConfigRevision
	run.Kind, run.Status = protocol.ConversationRunKindTask, protocol.ConversationStatusQueued
	run.Guidance = new(*item.Guidance)
	s.Page.Runs = append(s.Page.Runs, run)
	return s
}

func TestConversationGuidanceRecapture(t *testing.T) {
	for _, test := range []struct {
		name        string
		status      protocol.ConversationStatus
		detached    bool
		readdressed bool
	}{
		{name: "released", status: protocol.ConversationStatusQueued},
		{name: "claimed before refresh", status: protocol.ConversationStatusRunning},
		{name: "completed before refresh", status: protocol.ConversationStatusCompleted},
		{name: "readdressed before refresh", status: protocol.ConversationStatusQueued, readdressed: true},
		{name: "unobserved address and release", status: protocol.ConversationStatusQueued, detached: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			s := guidanceCaptureFixture()
			if test.detached {
				s.Page.Queue.Items[0].Guidance = nil
			}
			p := NewConversationProjection(s.Node, s.Page.Session, 1)
			if _, err := p.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: s}); err != nil {
				t.Fatal(err)
			}
			run := copyValue(s.Page.Runs[1])
			run.Revision += 2
			run.Status = test.status
			if test.readdressed {
				run.Guidance = new(protocol.TurnID{3})
			}
			if test.status != protocol.ConversationStatusQueued {
				run.StartedMs = new(int64(10))
			}
			if test.status == protocol.ConversationStatusCompleted {
				run.FinishedMs = new(int64(11))
			}
			if got, err := p.Apply(1, conversationFrame(*p.Snapshot(), "run", run)); got != Applied || err != nil {
				t.Fatalf("recaptured run: %v, %v", got, err)
			}
			queue := copyValue(s.Page.Queue)
			queue.Revision += 4
			queue.Items[0].Revision += 3
			queue.Items[0].ConfigRevision = run.Revision
			queue.Items[0].Guidance = nil
			if test.readdressed {
				queue.Items[0].Guidance = new(*run.Guidance)
			}
			if test.status != protocol.ConversationStatusQueued {
				queue.Items = nil
			}
			if got, err := p.Apply(1, conversationFrame(*p.Snapshot(), "queue", queue)); got != Applied || err != nil {
				t.Fatalf("recaptured queue: %v, %v", got, err)
			}
			if !reflect.DeepEqual(p.snapshot.Page.Queue, queue) || !reflect.DeepEqual(p.snapshot.Page.Runs[1], run) {
				t.Fatal("recapture did not update both published captures")
			}
		})
	}
}

func TestConversationGuidanceCaptureGuards(t *testing.T) {
	for _, test := range []struct {
		name   string
		change func(*protocol.ConversationRun, *protocol.ConversationRun)
	}{
		{"ordinary queued run", func(previous, _ *protocol.ConversationRun) { previous.Guidance = nil }},
		{"running capture", func(previous, _ *protocol.ConversationRun) { previous.Status = protocol.ConversationStatusRunning }},
		{"finished capture", func(previous, _ *protocol.ConversationRun) { previous.Status = protocol.ConversationStatusCompleted }},
		{"started capture", func(previous, _ *protocol.ConversationRun) { previous.StartedMs = new(int64(1)) }},
		{"older capture", func(previous, current *protocol.ConversationRun) { current.Revision = previous.Revision - 1 }},
		{"changed worktree", func(_, current *protocol.ConversationRun) { current.Worktree = protocol.WorktreeID{9} }},
	} {
		t.Run(test.name, func(t *testing.T) {
			s := guidanceCaptureFixture()
			run := copyValue(s.Page.Runs[1])
			run.Revision++
			test.change(&s.Page.Runs[1], &run)
			p := NewConversationProjection(s.Node, s.Page.Session, 1)
			if _, err := p.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: s}); err != nil {
				t.Fatal(err)
			}
			before := p.Snapshot()
			if got, err := p.Apply(1, conversationFrame(s, "run", run)); got != Ignored || faultCode(err) != protocol.ErrorCodeInvalidRequest || !reflect.DeepEqual(before, p.Snapshot()) {
				t.Fatalf("invalid recapture changed the projection: %v, %v", got, err)
			}
		})
	}
	for _, versioned := range []bool{false, true} {
		s := guidanceCaptureFixture()
		p := NewConversationProjection(s.Node, s.Page.Session, 1)
		if _, err := p.Apply(1, protocol.Update{Kind: "conversation_snapshot", Data: s}); err != nil {
			t.Fatal(err)
		}
		queue := copyValue(s.Page.Queue)
		queue.Revision++
		queue.Items[0].ConfigRevision++
		if versioned {
			queue.Items[0].Revision++
			queue.Items[0].ConfigRevision = s.Page.Queue.Items[0].ConfigRevision - 1
		}
		before := p.Snapshot()
		if got, err := p.Apply(1, conversationFrame(s, "queue", queue)); got != Ignored || faultCode(err) != protocol.ErrorCodeInvalidRequest || !reflect.DeepEqual(before, p.Snapshot()) {
			t.Fatalf("unversioned or regressed queue capture changed state (versioned=%v): %v, %v", versioned, got, err)
		}
	}
}

func TestConversationGuidanceCaptureHistoryMerge(t *testing.T) {
	for _, keep := range []bool{false, true} {
		s := guidanceCaptureFixture()
		older := copyValue(s.Page)
		newer := copyValue(older)
		newer.Runs[1].Revision++
		newer.Runs[1].Status = protocol.ConversationStatusRunning
		newer.Runs[1].StartedMs = new(int64(1))
		current, incoming := older, newer
		if keep {
			current, incoming = newer, older
		}
		if err := mergeConversationPage(&current, incoming, keep, nil); err != nil {
			t.Fatalf("merge recaptured guidance (keep=%v): %v", keep, err)
		}
		if !reflect.DeepEqual(current.Runs, newer.Runs) {
			t.Fatalf("merge replaced the latest capture (keep=%v)", keep)
		}
		before := copyValue(current)
		if err := mergeConversationPage(&current, older, false, nil); faultCode(err) != protocol.ErrorCodeInvalidRequest || !reflect.DeepEqual(current, before) {
			t.Fatalf("older capture replaced the current run: %v", err)
		}
	}
}
