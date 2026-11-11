package node

import (
	"context"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRemovedSessionStopsCommands(t *testing.T) {
	t.Parallel()
	stopped := make(chan protocol.SessionID, 2)
	service, err := process.New(process.Options{Profile: t.TempDir(), Run: func(ctx context.Context, launch process.Launch, _ func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
		<-ctx.Done()
		stopped <- launch.Session
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "cancelled"}}, nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = service.Close() })
	node := &Node{processes: service, ctx: context.Background()}
	removed, kept := protocol.NewSessionID(), protocol.NewSessionID()
	for _, session := range []protocol.SessionID{removed, kept} {
		if _, err := service.Start(process.Launch{ID: protocol.NewRequestID(), Session: session, Turn: protocol.NewTurnID(), Root: t.TempDir(), Command: "fixture", Access: protocol.ProcessAccessWorkspace, TimeoutMs: 1}); err != nil {
			t.Fatal(err)
		}
	}
	node.processSessionEvents([]protocol.Event{{Kind: "session_changed"}, {Kind: "sessions_removed", Data: []protocol.SessionID{removed}}}, func([]protocol.SessionID) {})
	select {
	case session := <-stopped:
		if session != removed {
			t.Fatal("removal stopped another session's command")
		}
	case <-time.After(10 * time.Second):
		t.Fatal("removed session kept its command")
	}
	updates, release := service.Subscribe(removed)
	defer release()
	deadline := time.After(10 * time.Second)
	for items := <-updates; len(items) != 0; {
		select {
		case items = <-updates:
		case <-deadline:
			t.Fatal("removed session kept its command entry")
		}
	}
	if items := service.List(kept); len(items) != 1 || items[0].Status.Kind != "running" {
		t.Fatal("removal changed another session's command", items)
	}
	select {
	case <-stopped:
		t.Fatal("removal stopped another session's command")
	default:
	}
}
