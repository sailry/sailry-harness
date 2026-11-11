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
				count := 100
				if scenario == "retained gap resets" {
					count = maxRetained + 1
				}
				for i := 0; i < count; i++ {
					h.Publish(session, protocol.ConversationChange{Kind: "queue", Data: protocol.DefaultConversationQueue()})
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
					sealed := h.Seal(session, turn)
					if len(sealed) != 1 {
						t.Fatal("partial was not available for sealing")
					}
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
