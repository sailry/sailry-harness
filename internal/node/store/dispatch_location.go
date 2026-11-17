package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Dispatch is stored and run on the selected execution Node. A caller selects
// that Node before invoking a handler; a record cannot redirect its callbacks.
func bindDispatchLocation(node protocol.NodeID, value any) error {
	var location **protocol.NodeID
	switch record := value.(type) {
	case *protocol.DispatchHandler:
		location = &record.ExecutionNode
	case *protocol.DispatchSchedule:
		location = &record.ExecutionNode
	case *protocol.DispatchJob:
		location = &record.ExecutionNode
	default:
		return nil
	}
	if *location != nil && **location != node {
		return fault(protocol.ErrorCodeWrongTarget, "dispatch belongs to Node "+(**location).String())
	}
	if *location == nil {
		*location = &node
	}
	return nil
}

func bindStoredDispatchLocation(ctx context.Context, db *sql.Conn, value any) error {
	node, err := storageNode(ctx, db)
	if err != nil {
		return err
	}
	return bindDispatchLocation(node, value)
}
