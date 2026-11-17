package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// awaitControllerLoginReplacement bridges login completion and the existing
// configuration push. It runs before admission, so an absent cache version
// cannot become a permanently recorded failed recovery request.
func (node *Node) awaitControllerLoginReplacement(ctx context.Context, caller protocol.NodeID, input protocol.CommandResumeControllerLoginPayload) error {
	for {
		node.auth.mu.Lock()
		changed, closed := node.auth.changed, node.auth.closed
		node.auth.mu.Unlock()
		if closed {
			return context.Canceled
		}
		available := false
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			if _, err := store.CheckControllerLoginWait(ctx, db, node.ID(), caller, input); err != nil {
				return err
			}
			var err error
			available, err = store.ControllerLoginReplacementAvailable(ctx, db, node.ID(), input)
			return err
		})
		if err != nil || available {
			return err
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-changed:
		}
	}
}
