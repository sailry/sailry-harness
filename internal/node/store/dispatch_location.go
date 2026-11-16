package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Dispatch is stored and run on the selected execution Node. A caller selects
// that Node before invoking a handler; a record cannot redirect its callbacks.
func dispatchNode(ctx context.Context, db *sql.Conn) (protocol.NodeID, error) {
	var identity []byte
	if err := db.QueryRowContext(ctx, "SELECT identity FROM node WHERE singleton=1").Scan(&identity); err != nil {
		return protocol.NodeID{}, err
	}
	if len(identity) != len(protocol.NodeID{}) {
		return protocol.NodeID{}, errors.New("invalid dispatch Node identity")
	}
	return protocol.NodeID(identity), nil
}

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
	node, err := dispatchNode(ctx, db)
	if err != nil {
		return err
	}
	return bindDispatchLocation(node, value)
}
