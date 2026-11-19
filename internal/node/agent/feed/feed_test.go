package feed

import (
	"context"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConversationFeed(t *testing.T) {
	for _, scenario := range []string{"reservation precedes commit", "retained lag recovery", "retained gap resets", "committed supersedes draft", "stop seals late partials", "draft bytes are bounded", "a long turn keeps streaming"} {
		t.Run(scenario, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), time.Second)
			defer cancel()
			var node protocol.NodeID
			node[0] = 1
			session, turn := protocol.NewSessionID(), protocol.NewTurnID()
			h := New(node)
			nodeClosed := make(chan struct{})
			reservation := h.Reserve(session, nodeClosed)
			defer reservation.Close()
			run := protocol.DefaultConversationRun()
			run.Session = session
			run.Turn = turn
			run.Worktree = protocol.NewWorktreeID()
			run.Status = protocol.ConversationStatusRunning
			run.Kind = protocol.ConversationRunKindTask
			h.Publish(session, protocol.ConversationChange{Kind: "run", Data: run})
			snapshot := protocol.DefaultConversationSnapshot()
			snapshot.Page.Session = session
			stream := reservation.Subscribe(snapshot)
			update, err := stream.Next(ctx)
			if err != nil || update.Kind != "conversation_snapshot" {
				t.Fatal("initial snapshot missing")
			}
			next := func() protocol.Update {
				t.Helper()
				update, err := stream.Next(ctx)
				if err != nil {
					t.Fatal(err)
				}
				return update
			}
			switch scenario {
			case "reservation precedes commit":
				frame, _ := payload[protocol.ConversationFrame](next().Data)
				if frame.Sequence != 1 || frame.Change.Kind != "run" {
					t.Fatal("commit was lost between reservation and snapshot")
				}
			case "retained lag recovery", "retained gap resets":
				var evictedQueue []protocol.Update
				var evictedRetained []retainedFrame
				count := 100
				if scenario == "retained gap resets" {
					count = maxRetained + 1
				}
				for i := 0; i < count; i++ {
					if queue := reservation.subscription.queue; len(queue) == liveCapacity && evictedQueue == nil {
						evictedQueue = queue[:1]
					}
					if replay := h.sessions[session].retained; len(replay) == maxRetained && evictedRetained == nil {
						evictedRetained = replay[:1]
					}
					h.Publish(session, protocol.ConversationChange{Kind: "queue", Data: protocol.DefaultConversationQueue()})
				}
				if len(evictedQueue) != 1 || evictedQueue[0].Data != nil ||
					scenario == "retained gap resets" && (len(evictedRetained) != 1 || evictedRetained[0].frame.Change.Data != nil) {
					t.Fatal("evicted frames still have payload references in the backing slice")
				}
				if scenario == "retained gap resets" {
					if next().Kind != "reset_required" {
						t.Fatal("unrecoverable gap did not reset")
					}
				} else {
					for i := uint64(1); i <= uint64(count+1); i++ {
						frame, ok := payload[protocol.ConversationFrame](next().Data)
						if !ok || frame.Sequence != i {
							t.Fatalf("retained order=%d, want %d", frame.Sequence, i)
						}
					}
				}
			case "a long turn keeps streaming":
				entry := protocol.DefaultConversationEntry()
				entry.Turn = turn
				for i := range maxFinalized + 2 {
					entry.ID = fmt.Sprint("entry-", i)
					h.Publish(session, protocol.ConversationChange{Kind: "entry", Data: entry})
				}
				before := h.Sequence(session)
				// The newest committed entry still drops its late delta.
				if err = h.Delta(session, protocol.ConversationDraft{ID: entry.ID, Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "late"}}}); err != nil || h.Sequence(session) != before {
					t.Fatalf("late delta revived a committed entry: %v", err)
				}
				if err = h.Delta(session, protocol.ConversationDraft{ID: "reply", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "partial"}}}); err != nil || h.Sequence(session) != before+1 {
					t.Fatalf("streaming stopped after many committed entries: %v", err)
				}
				h.mu.Lock()
				remembered, ordered := len(h.sessions[session].finalized), len(h.sessions[session].finalizedOrder)
				h.mu.Unlock()
				if remembered != maxFinalized || ordered != maxFinalized {
					t.Fatalf("committed entries remembered: %d, %d", remembered, ordered)
				}
			case "committed supersedes draft", "stop seals late partials", "draft bytes are bounded":
				draft := protocol.ConversationDraft{ID: "reply", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "partial"}}}
				if err = h.Delta(session, draft); err != nil {
					t.Fatal(err)
				}
				if scenario == "committed supersedes draft" {
					entry := protocol.DefaultConversationEntry()
					entry.ID = draft.ID
					entry.Turn = turn
					h.Publish(session, protocol.ConversationChange{Kind: "entry", Data: entry})
					if err = h.Delta(session, draft); err != nil {
						t.Fatal(err)
					}
					reserved := h.Reserve(session, nodeClosed)
					defer reserved.Close()
					if len(reserved.Drafts) != 0 {
						t.Fatal("committed draft was retained or revived")
					}
				}
				if scenario == "stop seals late partials" {
					before := h.Sequence(session)
					h.Seal(session, turn)
					_ = h.Delta(session, draft)
					if h.Sequence(session) != before {
						t.Fatal("late partial escaped sealing")
					}
				}
				if scenario == "draft bytes are bounded" {
					draft.Parts[0].Data = strings.Repeat("x", maxDraftBytes)
					before := h.Sequence(session)
					if h.Delta(session, draft) == nil || h.Sequence(session) != before {
						t.Fatal("oversized draft mutated the feed")
					}
				}
			}
		})
	}
}

func TestClosedConversationFeedReleasesUnreadFrames(t *testing.T) {
	h := New(protocol.NodeID{1})
	session, turn := protocol.NewSessionID(), protocol.NewTurnID()
	reservation := h.Reserve(session, make(chan struct{}))
	stream := reservation.Subscribe(protocol.DefaultConversationSnapshot()).(*Subscription)
	run := protocol.DefaultConversationRun()
	run.Session, run.Turn, run.Worktree = session, turn, protocol.NewWorktreeID()
	run.Status, run.Kind = protocol.ConversationStatusRunning, protocol.ConversationRunKindTask
	h.Publish(session, protocol.ConversationChange{Kind: "run", Data: run})
	if !stream.resume() || len(stream.pending) == 0 {
		t.Fatal("fixture did not hold a replay")
	}
	h.Publish(session, protocol.ConversationChange{Kind: "queue", Data: protocol.DefaultConversationQueue()})
	if stream.initial == nil || len(stream.queue) == 0 {
		t.Fatal("fixture did not hold unread updates")
	}
	if err := stream.Close(); err != nil {
		t.Fatal(err)
	}
	if stream.initial != nil || stream.queue != nil || stream.pending != nil {
		t.Fatal("closed observer keeps unread updates")
	}
	active := h.sessions[session]
	if active == nil || active.active == nil || active.retained != nil {
		t.Fatal("last observer did not release replay while leaving the active turn")
	}
	run.Status = protocol.ConversationStatusCompleted
	h.Publish(session, protocol.ConversationChange{Kind: "run", Data: run})
	if len(h.Sessions()) != 0 {
		t.Fatal("completed unobserved feed is still owned by the hub")
	}
	idle := h.Reserve(session, make(chan struct{}))
	if idle.Sequence != 0 || len(idle.Drafts) != 0 {
		t.Fatal("reopened feed inherited settled replay or drafts")
	}
	_ = idle.Close()
	if len(h.Sessions()) != 0 {
		t.Fatal("closed idle reservation retained the feed")
	}
}

func TestBackgroundFeedDeduplicationFollowsCurrentPage(t *testing.T) {
	h := New(protocol.NodeID{1})
	session := protocol.NewSessionID()
	reservation := h.Reserve(session, make(chan struct{}))
	defer reservation.Close()
	first := protocol.DefaultConversationBackgroundTask()
	first.ID = "first"
	first.Session, first.Turn = session, protocol.NewTurnID()
	first.Kind = protocol.ConversationBackgroundKind{Kind: "command", Data: protocol.ConversationBackgroundKindCommandPayload{ID: protocol.NewRequestID()}}
	first.State = protocol.ConversationBackgroundStateRunning
	second := first
	second.ID = "second"
	h.PublishBackground(session, []protocol.ConversationBackgroundTask{first, second})
	before := h.Sequence(session)
	h.PublishBackground(session, []protocol.ConversationBackgroundTask{second})
	if len(h.sessions[session].background) != 1 || h.Sequence(session) != before {
		t.Fatal("published page did not drop obsolete deduplication state")
	}
	second.Revision++
	h.PublishBackground(session, []protocol.ConversationBackgroundTask{second})
	if h.Sequence(session) != before+1 {
		t.Fatal("changed background task revision was not published")
	}
}
