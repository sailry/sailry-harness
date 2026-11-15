package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) replayAdmission(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, bool, error) {
	if err := node.begin(); err != nil {
		return link.Admission{}, false, err
	}
	defer node.work.Done()
	var previous *link.Response
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		previous, err = admission.Existing(ctx, db, caller, request, body)
		if err == nil && previous != nil && previous.Fault == nil {
			outcome := protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &previous.Output}}
			filtered, filterErr := node.inspectOutcomeAccess(ctx, db, caller, request.ID, outcome)
			if filterErr != nil {
				return filterErr
			}
			if result, ok := filtered.Data.(protocol.Result[protocol.Output, protocol.Fault]); ok && result.Err != nil {
				previous = &link.Response{Fault: result.Err}
			}
		}
		return err
	})
	if err != nil || previous == nil {
		return link.Admission{}, false, nodeFault(err)
	}
	completion := make(chan link.Response, 1)
	completion <- *previous
	close(completion)
	return link.Admission{
		Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: completion,
	}, true, nil
}
