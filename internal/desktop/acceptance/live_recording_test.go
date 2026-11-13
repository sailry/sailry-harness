package acceptance

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"sync"
	"testing"
	"time"

	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type recordedLiveTransport struct {
	*liveFixtureTransport
	updates chan protocol.Update
}

func (transport *recordedLiveTransport) Subscribe(context.Context, protocol.Topic) (link.Subscription, error) {
	return &recordedLiveSubscription{updates: transport.updates, closed: make(chan struct{})}, nil
}

type recordedLiveSubscription struct {
	updates <-chan protocol.Update
	closed  chan struct{}
	once    sync.Once
}

func (subscription *recordedLiveSubscription) Next(ctx context.Context) (protocol.Update, error) {
	select {
	case update := <-subscription.updates:
		return update, nil
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case <-subscription.closed:
		return protocol.Update{}, context.Canceled
	}
}

func (subscription *recordedLiveSubscription) Close() error {
	subscription.once.Do(func() { close(subscription.closed) })
	return nil
}

func TestLiveRecordingKeepsStreamingFramesAtOneSequence(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	node, session, turn := protocol.NodeID{1}, protocol.SessionID{2}, protocol.TurnID{3}
	directory := filepath.Join(t.TempDir(), "acceptance")
	if err := core.Own(directory, node, session); err != nil {
		t.Fatal(err)
	}
	recorder, err := core.NewRecorder(directory, node, session)
	if err != nil {
		t.Fatal(err)
	}
	transport := &recordedLiveTransport{updates: make(chan protocol.Update, 2), liveFixtureTransport: &liveFixtureTransport{
		dispatch: func(context.Context, protocol.Request) (protocol.Output, error) {
			t.Error("recording a Client snapshot must not execute a command")
			return protocol.Output{}, errors.New("recording fixture has no commands")
		},
	}}
	controller := client.New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	changed := make(chan struct{}, 1)
	release, err := controller.WatchConversation(ctx, session, func() {
		select {
		case changed <- struct{}{}:
		default:
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	w := &liveWindow{controller: controller, session: protocol.Session{ID: session}, recorder: recorder, started: time.Now()}
	for _, text := range []string{"First", "First streaming continuation"} {
		snapshot := protocol.DefaultConversationSnapshot()
		snapshot.Node, snapshot.Sequence, snapshot.Page.Session = node, 1, session
		snapshot.Page.Revision = 1
		run := protocol.DefaultConversationRun()
		run.Worktree, run.Session, run.Turn = protocol.WorktreeID{4}, session, turn
		run.Sequence, run.Revision = 1, 1
		run.Kind, run.Status = protocol.ConversationRunKindTask, protocol.ConversationStatusRunning
		snapshot.Page.Runs = []protocol.ConversationRun{run}
		snapshot.Drafts = []protocol.ConversationDraft{{ID: "fixture-reply", Turn: turn, Author: "assistant", Branch: "main", Parts: []protocol.ConversationPart{{Kind: "text", Data: text}}}}
		// Match the wire representation, including decoded union payloads, and
		// report incomplete fixture fields before they reach the live recorder.
		encoded, err := json.Marshal(snapshot)
		if err != nil {
			t.Fatalf("encode recording fixture: %v", err)
		}
		if err := json.Unmarshal(encoded, &snapshot); err != nil {
			t.Fatalf("decode recording fixture: %v", err)
		}
		transport.updates <- protocol.Update{Kind: "conversation_snapshot", Data: snapshot}
		for {
			state, known := controller.Conversation(session)
			if known && state.Err != nil {
				t.Fatal(state.Err)
			}
			if known && state.Connected && len(state.Snapshot.Drafts) == 1 && len(state.Snapshot.Drafts[0].Parts) == 1 {
				value, _ := copyLiveValue[string](state.Snapshot.Drafts[0].Parts[0].Data)
				if value == text {
					break
				}
			}
			select {
			case <-changed:
			case <-ctx.Done():
				t.Fatal(ctx.Err())
			}
		}
		w.recordFrame()
		w.recordFrame() // an unchanged rendered frame must not duplicate evidence
	}
	if err := w.flushRecording(); err != nil {
		// Only this test-owned snapshot may expose the recorder's raw error;
		// production reports retain the safe live UI failure description.
		state, _ := controller.Conversation(session)
		cause := recorder.Append(core.RecordingFrame{At: time.Since(w.started), Snapshot: state.Snapshot})
		t.Fatalf("%v; fixture frame error: %v", err, cause)
	}
	recording, err := core.ReadRecording(directory, session)
	if err != nil {
		t.Fatal(err)
	}
	if len(recording.Frames) != 2 || recording.Truncated || recording.Frames[0].Snapshot.Sequence != recording.Frames[1].Snapshot.Sequence {
		t.Fatalf("streaming samples were lost or duplicated: %+v", recording)
	}
	first, _ := copyLiveValue[string](recording.Frames[0].Snapshot.Drafts[0].Parts[0].Data)
	last, _ := copyLiveValue[string](recording.Frames[1].Snapshot.Drafts[0].Parts[0].Data)
	if first != "First" || last != "First streaming continuation" {
		t.Fatal("recording did not keep independent original streaming snapshots")
	}
}
