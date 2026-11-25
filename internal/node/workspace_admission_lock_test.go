package node

import (
	"context"
	"database/sql"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A workspace command waiting for its admission write does not hold the Node
// mutex, so a slow store write does not stall every request and
// subscription that begins meanwhile.
func TestWorkspaceAdmissionDoesNotHoldTheNode(t *testing.T) {
	node := nodeFixture(t)
	session := sessionFixture(t, node)
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
	dispatched := make(chan error, 1)
	go func() {
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: session.Worktree, Path: "admitted.txt", Text: "written\n"}})
		accepted, err := node.Dispatch(context.Background(), node.ID(), request)
		if err == nil {
			if response, ok := <-accepted.Completion; ok && response.Fault != nil {
				err = response.Fault
			}
		}
		dispatched <- err
	}()
	// The command reaches its admission write and waits behind the held one.
	time.Sleep(200 * time.Millisecond)
	select {
	case err := <-dispatched:
		t.Fatalf("command finished while the store writer was held: %v", err)
	default:
	}
	if !node.mu.TryLock() {
		release()
		<-dispatched
		t.Fatal("a workspace admission held the Node mutex while it waited for the store")
	}
	node.mu.Unlock()
	release()
	if err := <-dispatched; err != nil {
		t.Fatal(err)
	}
}
