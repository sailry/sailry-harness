package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Check a historical remote replacement before admission so that a missing
// cache entry does not permanently record a failed recovery request.
func (node *Node) checkControllerLoginReplacement(ctx context.Context, caller protocol.NodeID, input protocol.CommandResumeControllerLoginPayload) error {
	return node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		if _, err := store.CheckControllerLoginWait(ctx, db, node.ID(), caller, input); err != nil {
			return err
		}
		available, err := store.ControllerLoginReplacementAvailable(ctx, db, node.ID(), input)
		if err != nil || available {
			return err
		}
		return protocol.NewConfigurationFault(protocol.ConfigurationRuntimeCredentialMissing, "the captured account replacement is unavailable on this Node")
	})
}
