package node

import (
	"context"
	"database/sql"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A durable command waiting for its admission write does not hold the Node
// mutex, whether it executes during admission or in a lane, so a slow store
// write does not stall every request and subscription that begins meanwhile.
func TestCommandAdmissionDoesNotHoldTheNode(t *testing.T) {
	node := nodeFixture(t)
	renamed, removed := sessionFixture(t, node), sessionFixture(t, node)
	holding, gate := make(chan struct{}), make(chan struct{})
	held := make(chan error, 1)
	go func() {
		held <- node.profile.Store.Node.Write(context.Background(), func(*sql.Conn) error {
			close(holding)
			<-gate
			return nil
		})
	}()
	<-holding
	released := false
	release := func() {
		if !released {
			released = true
			close(gate)
			if err := <-held; err != nil {
				t.Error(err)
			}
		}
	}
	defer release()
	commands := []protocol.Command{
		{Kind: "rename_session", Data: protocol.CommandRenameSessionPayload{Session: renamed.ID, ExpectedRevision: renamed.Revision, Title: "Renamed"}},
		{Kind: "remove_session", Data: protocol.CommandRemoveSessionPayload{Session: removed.ID, ExpectedRevision: removed.Revision}},
	}
	dispatched := make(chan error, len(commands))
	for _, command := range commands {
		go func() {
			accepted, err := node.Dispatch(context.Background(), node.ID(), protocol.NewRequest(node.ID(), command))
			if err == nil {
				if response, ok := <-accepted.Completion; ok && response.Fault != nil {
					err = response.Fault
				}
			}
			dispatched <- err
		}()
	}
	// Both commands reach their admission writes and wait behind the held one.
	time.Sleep(200 * time.Millisecond)
	select {
	case err := <-dispatched:
		t.Fatalf("a command finished while the store writer was held: %v", err)
	default:
	}
	if !node.mu.TryLock() {
		release()
		t.Fatal("a command admission held the Node mutex while it waited for the store")
	}
	node.mu.Unlock()
	release()
	for range commands {
		if err := <-dispatched; err != nil {
			t.Fatal(err)
		}
	}
}
