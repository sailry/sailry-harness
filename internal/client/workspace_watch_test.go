package client

import (
	"context"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type sharedResourceTransport struct {
	clientTransport
	muResource   sync.Mutex
	topics       []protocol.Topic
	subscription *clientSubscription
}

func (t *sharedResourceTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	t.muResource.Lock()
	defer t.muResource.Unlock()
	t.topics = append(t.topics, topic)
	return t.subscription, nil
}
func TestSharedWorkspaceAndCommandOwners(t *testing.T) {
	for _, kind := range []string{"files", "repository", "commands"} {
		t.Run(kind, func(t *testing.T) {
			subscription := &clientSubscription{updates: make(chan protocol.Update, 4), closed: make(chan struct{})}
			transport := &sharedResourceTransport{subscription: subscription}
			controller := New(transport)
			defer controller.Close()
			worktree := protocol.NewWorktreeID()
			session := protocol.NewSessionID()
			one, two := make(chan struct{}, 8), make(chan struct{}, 8)
			observe := func(ch chan struct{}) (func(), error) {
				if kind == "commands" {
					return controller.ObserveCommands(context.Background(), session, func() { ch <- struct{}{} })
				}
				return controller.ObserveWorkspace(context.Background(), worktree, kind == "repository", func(protocol.Update) { ch <- struct{}{} })
			}
			releaseOne, err := observe(one)
			if err != nil {
				t.Fatal(err)
			}
			releaseTwo, err := observe(two)
			if err != nil {
				t.Fatal(err)
			}
			update := protocol.Update{Kind: "files_changed", Data: protocol.UpdateFilesChangedPayload{Node: transport.Target(), Worktree: worktree, References: 4}}
			if kind == "commands" {
				update = protocol.Update{Kind: "commands", Data: protocol.UpdateCommandsPayload{Node: transport.Target(), Session: session, Items: []protocol.ProcessInfo{{ID: protocol.NewRequestID(), Session: session, Command: "echo ready", Status: protocol.ProcessStatus{Kind: "running"}}}}}
			}
			subscription.updates <- update
			<-one
			<-two
			transport.muResource.Lock()
			count := len(transport.topics)
			transport.muResource.Unlock()
			if count != 1 {
				t.Fatalf("shared observer created %d subscriptions", count)
			}
			if kind == "commands" {
				state, ok := controller.Commands(session)
				if !ok || !state.Connected || len(state.Items) != 1 {
					t.Fatal("command snapshot lost", state)
				}
			} else {
				state, ok := controller.Workspace(worktree, kind == "repository")
				if !ok || !state.Connected || state.References != 1 {
					t.Fatal("workspace references lost", state)
				}
			}
			releaseOne()
			subscription.updates <- update
			<-two
			select {
			case <-one:
				t.Fatal("released observer was called")
			default:
			}
			select {
			case <-subscription.closed:
				t.Fatal("shared owner closed before final observer")
			default:
			}
			releaseTwo()
			<-subscription.closed
			if kind == "commands" {
				if _, ok := controller.Commands(session); ok {
					t.Fatal("unobserved command watch was retained")
				}
			} else if _, ok := controller.Workspace(worktree, kind == "repository"); ok {
				t.Fatal("unobserved workspace watch was retained")
			}
		})
	}
	t.Run("command_output", func(t *testing.T) {
		session, command := protocol.NewSessionID(), protocol.NewRequestID()
		transport := &clientTransport{dispatch: func(request protocol.Request) (link.Admission, error) {
			snapshot := protocol.DefaultProcessSnapshot()
			snapshot.Info.ID, snapshot.Info.Session, snapshot.Info.Status.Kind = command, session, "exited"
			completion := make(chan link.Response, 1)
			completion <- link.Response{Output: protocol.Output{Kind: "command_output", Data: snapshot}}
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
		}}
		controller := New(transport)
		defer controller.Close()
		changed := make(chan struct{}, 4)
		release, err := controller.ObserveCommandOutput(context.Background(), session, command, func() { changed <- struct{}{} })
		if err != nil {
			t.Fatal(err)
		}
		<-changed
		if state, ok := controller.CommandOutput(session, command); !ok || !state.Known || state.Err != nil {
			t.Fatal("finished command output is unavailable to its observer", state.Err)
		}
		release()
		if _, ok := controller.CommandOutput(session, command); ok {
			t.Fatal("unobserved command output was retained")
		}
	})
}
