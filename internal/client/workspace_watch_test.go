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
		})
	}
}
