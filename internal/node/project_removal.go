package node

import (
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// closeProjectResources closes what a project removal promises to close: the
// project's running terminals and the background commands of its sessions. The
// removal is validated first, so a stale or busy one closes nothing. It reports
// whether a terminal was closed, because a removal that fails afterwards must
// say so.
func (node *Node) closeProjectResources(command protocol.Command) (bool, error) {
	input, ok := command.Data.(*protocol.CommandRemoveProjectPayload)
	if !ok {
		return false, nil
	}
	var plan store.ProjectRemoval
	if err := node.profile.Store.Node.Read(node.ctx, func(db *sql.Conn) error {
		var err error
		plan, err = store.PlanProjectRemoval(node.ctx, db, input.Expected)
		return err
	}); err != nil {
		return false, err
	}
	closed := false
	for _, terminal := range plan.Terminals {
		// A record without a terminal in this process is closed with the removal.
		_, err := node.terminals.CloseTerminal(terminal.Worktree, terminal.ID)
		var failure *protocol.Fault
		if errors.As(err, &failure) && failure.Code == protocol.ErrorCodeNotFound {
			continue
		}
		if err != nil {
			if closed && failure != nil {
				marked := *failure
				marked.Details = &protocol.FaultDetails{Kind: "project_removal", Data: protocol.FaultDetailsProjectRemovalPayload{TerminalsMayBeClosed: true}}
				return closed, &marked
			}
			return closed, err
		}
		closed = true
	}
	for _, session := range plan.Sessions {
		node.processes.StopAll(session)
	}
	return closed, nil
}
