package node

import (
	"github.com/sailry/sailry-harness/internal/protocol"
)

// processSessionEvents stops the commands of removed sessions. The store has
// already dismissed their evidence in the removing transaction.
func (node *Node) processSessionEvents(changes []protocol.Event) {
	for _, change := range changes {
		if change.Kind != "sessions_removed" {
			continue
		}
		removed, valid := dispatchPayload[[]protocol.SessionID](change.Data)
		if !valid {
			continue
		}
		// Closing waits for each command's completion write, and the command
		// lock may be held here, so it runs on its own. Node closure stops
		// every command itself.
		go func() {
			for _, session := range removed {
				_, _ = node.processes.CloseSession(node.ctx, session)
			}
		}()
	}
}
