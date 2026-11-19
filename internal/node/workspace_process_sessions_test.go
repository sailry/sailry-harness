package node

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPromotedCommandNoticeRespectsDelegation(t *testing.T) {
	t.Parallel()
	node := &Node{}
	request := protocol.Request{Command: protocol.Command{Kind: "run_command", Data: &protocol.CommandRunCommandPayload{}}}
	info := protocol.DefaultProcessInfo()
	info.Status.Kind = "running"
	output := protocol.Output{Kind: "command_started", Data: info}
	var delegation *protocol.Delegation
	for _, session := range []protocol.Session{{}, {Delegation: &delegation}} {
		value, _ := node.workspaceToolValue(session, request, output).(map[string]any)
		data, _ := value["data"].(map[string]any)
		notice, _ := data["notice"].(string)
		child := session.Delegation != nil
		if !strings.Contains(notice, "read_command") || strings.Contains(notice, "returning to the parent") != child ||
			strings.Contains(notice, "reported automatically") == child {
			t.Fatalf("delegated=%v received notice %q", child, notice)
		}
	}
}

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
	cleaned := make(chan struct{})
	for _, session := range []protocol.SessionID{removed, kept} {
		if _, err := service.Start(process.Launch{ID: protocol.NewRequestID(), Session: session, Turn: protocol.NewTurnID(), Root: t.TempDir(), Command: "fixture", Access: protocol.ProcessAccessWorkspace, TimeoutMs: 1}); err != nil {
			t.Fatal(err)
		}
	}
	node.processSessionEvents([]protocol.Event{{Kind: "session_changed"}, {Kind: "sessions_removed", Data: []protocol.SessionID{removed}}}, func([]protocol.SessionID) { <-cleaned })
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
	// Node close waits for the stop and for what follows it.
	closed := make(chan struct{})
	go func() { node.work.Wait(); close(closed) }()
	select {
	case <-closed:
		t.Fatal("Node close did not wait for the removed session's commands")
	case <-time.After(100 * time.Millisecond):
	}
	close(cleaned)
	select {
	case <-closed:
	case <-time.After(10 * time.Second):
		t.Fatal("the removed session's cleanup did not end")
	}
}
