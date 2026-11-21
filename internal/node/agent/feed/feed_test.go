package feed

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

func TestConversationFeed(t *testing.T) {
	for _, scenario := range []string{"reservation precedes commit", "retained lag recovery", "retained gap resets", "committed supersedes draft", "hidden draft stays removed", "stop seals late partials", "draft bytes are bounded", "a long turn keeps streaming"} {
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
					queue := protocol.DefaultConversationQueue()
					queue.Revision = uint64(i + 1)
					h.Publish(session, protocol.ConversationChange{Kind: "queue", Data: queue})
				}
				if scenario == "retained gap resets" {
					r := &h.sessions[session].replay
					if r.count != maxRetained || r.at(1) != nil || len(r.metadata) != 1 {
						t.Fatal("count eviction left old frames or metadata addressable")
					}
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
			case "committed supersedes draft", "hidden draft stays removed", "stop seals late partials", "draft bytes are bounded":
				draft := protocol.ConversationDraft{ID: "reply", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "partial"}}}
				if err = h.Delta(session, draft); err != nil {
					t.Fatal(err)
				}
				if scenario == "committed supersedes draft" || scenario == "hidden draft stays removed" {
					if scenario == "hidden draft stays removed" {
						h.Publish(session, protocol.ConversationChange{Kind: "hidden", Data: protocol.ConversationChangeHiddenPayload{ID: draft.ID, Turn: turn}})
					} else {
						entry := protocol.DefaultConversationEntry()
						entry.ID, entry.Turn = draft.ID, turn
						h.Publish(session, protocol.ConversationChange{Kind: "entry", Data: entry})
					}
					before := h.Sequence(session)
					if err = h.Delta(session, draft); err != nil || h.Sequence(session) != before {
						t.Fatalf("late delta escaped finalization: %v", err)
					}
					reserved := h.Reserve(session, nodeClosed)
					defer reserved.Close()
					if len(reserved.Drafts) != 0 {
						t.Fatal("draft was retained or revived")
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
	active := h.sessions[session]
	if stream.initial == nil || active.replay.count != 1 || len(active.replay.metadata) != 1 {
		t.Fatal("fixture did not hold an unread initial snapshot and metadata frame")
	}
	if err := stream.Close(); err != nil {
		t.Fatal(err)
	}
	if stream.initial != nil {
		t.Fatal("closed observer keeps its initial snapshot")
	}
	if h.sessions[session] != active || active.active == nil || active.replay.frames != nil || active.replay.metadata != nil {
		t.Fatal("last observer did not release replay while leaving the active turn")
	}
	h.Publish(session, protocol.ConversationChange{Kind: "queue", Data: protocol.DefaultConversationQueue()})
	if active.replay.frames != nil {
		t.Fatal("unobserved active turn collected replay")
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

func nextUpdate(t *testing.T, stream link.Subscription, kind string) protocol.Update {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), time.Second)
	defer cancel()
	update, err := stream.Next(ctx)
	if err != nil || update.Kind != kind {
		t.Fatalf("update kind = %q, want %q: %v", update.Kind, kind, err)
	}
	return update
}

func nextFrame(t *testing.T, stream link.Subscription, sequence uint64, kind string) *protocol.ConversationFrame {
	t.Helper()
	update := nextUpdate(t, stream, "conversation_frame")
	frame, ok := update.Data.(*protocol.ConversationFrame)
	if !ok {
		t.Fatalf("conversation frame payload = %T", update.Data)
	}
	if frame.Sequence != sequence || frame.Change.Kind != kind {
		t.Fatalf("frame = %s/%d, want %s/%d", frame.Change.Kind, frame.Sequence, kind, sequence)
	}
	return frame
}

func feedSnapshot(session protocol.SessionID) protocol.ConversationSnapshot {
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Page.Session = session
	return snapshot
}

func assertReplayBound(t *testing.T, r *replay) (count, total int) {
	t.Helper()
	var ordinary, large int
	for _, item := range r.frames {
		if item.body == nil {
			continue
		}
		count++
		if size := cap(item.body); size <= maxRetainedBytes {
			ordinary += size
		} else {
			if large != 0 {
				t.Fatal("more than one large frame remains in replay")
			}
			large = size
		}
	}
	if count != r.count || count > maxRetained || ordinary != r.bytes || ordinary > maxRetainedBytes || large > protocol.MAX_FRAME_BYTES {
		t.Fatalf("replay bound violated: %d frames, %d ordinary bytes, %d large bytes", count, ordinary, large)
	}
	return count, ordinary + large
}

func TestConversationFeedByteBurstResetsOnlyTheLostCursor(t *testing.T) {
	node, session := protocol.NodeID{1}, protocol.NewSessionID()
	h := New(node)
	fast := h.Reserve(session, nil).Subscribe(feedSnapshot(session))
	defer fast.Close()
	nextUpdate(t, fast, "conversation_snapshot")
	slow := h.Reserve(session, nil)
	defer slow.Close()
	entry := protocol.DefaultConversationEntry()
	entry.Turn, entry.Author = protocol.NewTurnID(), "assistant"
	entry.Parts = []protocol.ConversationPart{{Kind: "text", Data: strings.Repeat("x", maxRetainedBytes/8)}}
	const burst = 12
	var publishedBytes int
	for i := uint64(1); i <= burst; i++ {
		entry.Sequence, entry.ID = i, fmt.Sprint(i)
		h.Publish(session, protocol.ConversationChange{Kind: "entry", Data: entry})
		publishedBytes += len(h.sessions[session].replay.at(i))
		frame := nextFrame(t, fast, i, "entry")
		if frame.Node != node || frame.Session != session || frame.Change.Data.(*protocol.ConversationEntry).ID != entry.ID {
			t.Fatal("fast reader lost frame scope or order")
		}
	}
	count, storedBytes := assertReplayBound(t, &h.sessions[session].replay)
	if publishedBytes <= maxRetainedBytes || count <= 0 || count >= burst || storedBytes > maxRetainedBytes {
		t.Fatalf("byte burst did not exercise eviction: published=%d/%d, retained=%d/%d", burst, publishedBytes, count, storedBytes)
	}
	t.Logf("burst: old 64-slot queue could hold all %d changes (%d JSON bytes before wrapping); replay holds %d changes (%d backing bytes)", burst, publishedBytes, count, storedBytes)

	stream := slow.Subscribe(feedSnapshot(session))
	if snapshot := nextUpdate(t, stream, "conversation_snapshot").Data.(protocol.ConversationSnapshot); snapshot.Sequence != 0 {
		t.Fatal("late initial read moved off the reservation's sequence")
	}
	nextUpdate(t, stream, "reset_required")
	if _, err := stream.Next(t.Context()); !isUnavailable(err) {
		t.Fatalf("lost cursor did not end after reset: %v", err)
	}

	resumed := h.Reserve(session, nil)
	defer resumed.Close()
	snapshot := feedSnapshot(session)
	entry.Parts = nil
	snapshot.Page.Entries = []protocol.ConversationEntry{entry}
	entry.Sequence, entry.ID = burst+1, "after-reserve"
	h.Publish(session, protocol.ConversationChange{Kind: "entry", Data: entry})
	fresh := resumed.Subscribe(snapshot)
	initial := nextUpdate(t, fresh, "conversation_snapshot").Data.(protocol.ConversationSnapshot)
	if initial.Sequence != burst || len(initial.Page.Entries) != 1 || initial.Page.Entries[0].Sequence != burst {
		t.Fatal("reconnect did not start at its fresh history snapshot")
	}
	nextFrame(t, fresh, burst+1, "entry")
	nextFrame(t, fast, burst+1, "entry")
}

func TestConversationFeedRetainsOneLargeLinkFrame(t *testing.T) {
	session := protocol.NewSessionID()
	h := New(protocol.NodeID{1})
	fast := h.Reserve(session, nil).Subscribe(feedSnapshot(session))
	defer fast.Close()
	slow := h.Reserve(session, nil).Subscribe(feedSnapshot(session))
	defer slow.Close()
	nextUpdate(t, fast, "conversation_snapshot")
	nextUpdate(t, slow, "conversation_snapshot")
	entry := protocol.DefaultConversationEntry()
	entry.Turn, entry.Author = protocol.NewTurnID(), "assistant"
	large := strings.Repeat("x", protocol.MAX_FRAME_BYTES-64*1024)
	publish := func(sequence uint64, text string) {
		entry.Sequence, entry.ID = sequence, fmt.Sprint(sequence)
		entry.Parts = []protocol.ConversationPart{{Kind: "text", Data: text}}
		h.Publish(session, protocol.ConversationChange{Kind: "entry", Data: entry})
	}
	publish(1, large)
	publish(2, "still contiguous")
	frame := nextFrame(t, fast, 1, "entry")
	if text, _ := payload[string](frame.Change.Data.(*protocol.ConversationEntry).Parts[0].Data); text != large {
		t.Fatal("large frame payload was changed")
	}
	update := protocol.Update{Kind: "conversation_frame", Data: frame}
	if err := wire.Write(io.Discard, wire.Reply{Kind: wire.ReplyUpdate, Update: &update}, false); err != nil {
		t.Fatalf("large frame should fit on Link: %v", err)
	}
	nextFrame(t, fast, 2, "entry")
	publish(3, strings.Repeat("x", maxRetainedBytes))
	r := &h.sessions[session].replay
	if count, size := assertReplayBound(t, r); count != 2 || size <= maxRetainedBytes || r.at(1) != nil || r.at(2) == nil {
		t.Fatalf("large frame replacement did not keep its contiguous suffix: %d frames, %d bytes", count, size)
	}
	nextFrame(t, fast, 3, "entry")
	nextUpdate(t, slow, "reset_required")
}

func TestConversationFeedDoesNotShareMutableReturnedParts(t *testing.T) {
	session := protocol.NewSessionID()
	h := New(protocol.NodeID{1})
	first := h.Reserve(session, nil).Subscribe(feedSnapshot(session))
	defer first.Close()
	second := h.Reserve(session, nil).Subscribe(feedSnapshot(session))
	defer second.Close()
	nextUpdate(t, first, "conversation_snapshot")
	nextUpdate(t, second, "conversation_snapshot")
	const original = `{"value":1}`
	part := json.RawMessage(original)
	entry := protocol.DefaultConversationEntry()
	entry.Turn, entry.ID, entry.Author, entry.Sequence = protocol.NewTurnID(), "mutable", "assistant", 1
	entry.Parts = []protocol.ConversationPart{{Kind: "resource", Data: part}}
	h.Publish(session, protocol.ConversationChange{Kind: "entry", Data: &entry})
	part[len(part)-2] = '2'
	readPart := func(stream link.Subscription) json.RawMessage {
		t.Helper()
		frame := nextFrame(t, stream, 1, "entry")
		value, _ := payload[json.RawMessage](frame.Change.Data.(*protocol.ConversationEntry).Parts[0].Data)
		if string(value) != original {
			t.Fatalf("replay was mutated: %s", value)
		}
		return value
	}
	readPart(first)[len(original)-2] = '3'
	readPart(second)
}

func isUnavailable(err error) bool {
	var fault *protocol.Fault
	return errors.As(err, &fault) && fault.Code == protocol.ErrorCodeUnavailable
}

func TestConversationFeedConcurrentNextAndClose(t *testing.T) {
	for _, stopNode := range []bool{false, true} {
		t.Run(fmt.Sprint("node_closed=", stopNode), func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				session, nodeClosed := protocol.NewSessionID(), make(chan struct{})
				h := New(protocol.NodeID{1})
				stream := h.Reserve(session, nodeClosed).Subscribe(feedSnapshot(session))
				defer stream.Close()
				nextUpdate(t, stream, "conversation_snapshot")
				type result struct {
					kind string
					err  error
				}
				read := make(chan result, 2)
				next := func() { update, err := stream.Next(context.Background()); read <- result{update.Kind, err} }
				go next()
				synctest.Wait()
				h.Publish(session, protocol.ConversationChange{Kind: "queue", Data: protocol.DefaultConversationQueue()})
				if got := <-read; got.err != nil || got.kind != "conversation_frame" {
					t.Fatalf("publication did not wake the waiting reader: %+v", got)
				}
				go next()
				synctest.Wait()
				go next()
				if stopNode {
					close(nodeClosed)
				} else if err := stream.Close(); err != nil {
					t.Fatal(err)
				}
				for range 2 {
					if got := <-read; !isUnavailable(got.err) {
						t.Fatalf("close did not wake a concurrent reader: %+v", got)
					}
				}
				if h.Observed(session) || len(h.Sessions()) != 0 {
					t.Fatal("closing did not release the session")
				}
			})
		})
	}
}

func TestConversationFeedSuppressesRepeatedMetadata(t *testing.T) {
	session := protocol.NewSessionID()
	h := New(protocol.NodeID{1})
	stream := h.Reserve(session, nil).Subscribe(feedSnapshot(session))
	defer stream.Close()
	nextUpdate(t, stream, "conversation_snapshot")
	const refreshes = 12
	changes := make([]protocol.ConversationChange, 0, 21)
	for i := range 20 {
		run := protocol.DefaultConversationRun()
		run.Session, run.Turn, run.Worktree = session, protocol.NewTurnID(), protocol.NewWorktreeID()
		run.Status, run.Kind = protocol.ConversationStatusCompleted, protocol.ConversationRunKindTask
		run.Sequence, run.Revision = uint64(i+1), 1
		changes = append(changes, protocol.ConversationChange{Kind: "run", Data: run})
	}
	changes = append(changes, protocol.ConversationChange{Kind: "queue", Data: protocol.DefaultConversationQueue()})
	for range refreshes {
		for _, change := range changes {
			h.Publish(session, change)
		}
	}
	h.Publish(session, protocol.ConversationChange{Kind: "statistics_failed", Data: protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture"}})
	var bytesOnFirstRefresh int
	for i, change := range changes {
		frame := nextFrame(t, stream, uint64(i+1), change.Kind)
		bytesOnFirstRefresh += len(encodeChange(frame.Change))
	}
	nextFrame(t, stream, uint64(len(changes)+1), "statistics_failed")
	t.Logf("%d refreshes: %d -> %d metadata frames; %d -> %d change JSON bytes (before Link wrapping)", refreshes, len(changes)*refreshes, len(changes), bytesOnFirstRefresh*refreshes, bytesOnFirstRefresh)
}

func TestConversationFeedMetadataChangesAndResync(t *testing.T) {
	session := protocol.NewSessionID()
	h := New(protocol.NodeID{1})
	stream := h.Reserve(session, nil).Subscribe(feedSnapshot(session))
	defer stream.Close()
	nextUpdate(t, stream, "conversation_snapshot")
	var sequence uint64
	emit := func(change protocol.ConversationChange) {
		t.Helper()
		h.Publish(session, change)
		sequence++
		frame := nextFrame(t, stream, sequence, change.Kind)
		if string(encodeChange(frame.Change)) != string(encodeChange(change)) {
			t.Fatalf("%s change lost fields at frame %d", change.Kind, sequence)
		}
	}
	run := protocol.DefaultConversationRun()
	run.Session, run.Turn, run.Worktree = session, protocol.NewTurnID(), protocol.NewWorktreeID()
	run.Kind, run.Status, run.Sequence, run.Revision = protocol.ConversationRunKindTask, protocol.ConversationStatusQueued, 1, 7
	runChange := protocol.ConversationChange{Kind: "run", Data: &run}
	emit(runChange)
	run.Status = protocol.ConversationStatusRunning
	emit(runChange)
	input := protocol.DefaultConversationSentInput()
	input.Message.Text = "sent text"
	run.Input = &input
	emit(runChange)
	run.Waiting = &protocol.ConversationRunWaiting{
		Kind: "controller_login", Attempt: protocol.NewRequestID(), Provider: protocol.NewProviderID(),
		Credential: protocol.CredentialRef{Node: protocol.NodeID{1}, ID: protocol.NewCredentialID()},
	}
	emit(runChange)
	run.Waiting.AccountIdentity = "recovery-account"
	emit(runChange)
	run.Waiting = nil
	emit(runChange) // previously seen body, but not the latest state
	run.Input = nil
	emit(runChange)
	run.Input = &input
	emit(runChange)
	entry := protocol.DefaultConversationEntry()
	entry.Turn, entry.ID, entry.Author, entry.Sequence = run.Turn, "input", "user", 1
	emit(protocol.ConversationChange{Kind: "entry", Data: entry})
	emit(protocol.ConversationChange{Kind: "hidden", Data: protocol.ConversationChangeHiddenPayload{Turn: entry.Turn, ID: entry.ID}})
	emit(runChange) // the canonical input no longer shadows this metadata
	queue := protocol.DefaultConversationQueue()
	queueChange := protocol.ConversationChange{Kind: "queue", Data: &queue}
	emit(queueChange)
	pause := protocol.ConversationQueuePauseUser
	queue.Paused, queue.PauseReason = true, &pause
	emit(queueChange) // revision alone does not establish equality

	joining := h.Reserve(session, nil)
	defer joining.Close()
	emit(runChange)
	emit(queueChange)
	joined := joining.Subscribe(feedSnapshot(session))
	if snapshot := nextUpdate(t, joined, "conversation_snapshot").Data.(protocol.ConversationSnapshot); snapshot.Sequence != sequence-2 {
		t.Fatal("new observer lost its reserved cursor")
	}
	nextFrame(t, joined, sequence-1, "run")
	nextFrame(t, joined, sequence, "queue")

	h.Reset(session)
	h.Publish(session, runChange)
	h.Publish(session, queueChange)
	if got := h.Sequence(session); got != sequence+3 {
		t.Fatalf("reset kept stale deduplication state: sequence=%d, want %d", got, sequence+3)
	}
	nextUpdate(t, stream, "reset_required")
	nextUpdate(t, joined, "reset_required")
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
