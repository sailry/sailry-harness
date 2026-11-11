package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) replayPluginAdmission(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, bool, error) {
	if err := node.begin(); err != nil {
		return link.Admission{}, false, err
	}
	defer node.work.Done()
	var previous *link.Response
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		previous, err = admission.Existing(ctx, db, caller, request, body)
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
