package store

import (
	"context"
	"database/sql"
	"reflect"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// SetRunCredentialWait changes only the running turn's projection; its capture,
// canonical records and already completed effects remain untouched.
func SetRunCredentialWait(ctx context.Context, db *sql.Conn, turn protocol.TurnID, waiting *protocol.ConversationRunWaiting) ([]protocol.Event, error) {
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return nil, err
	}
	if run.Status != protocol.ConversationStatusRunning || reflect.DeepEqual(run.Waiting, waiting) {
		return nil, nil
	}
	run.Waiting = waiting
	if err := putAgentRun(ctx, db, run); err != nil {
		return nil, err
	}
	return activityEvent(ctx, db, run.Session)
}
