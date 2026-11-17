package store

import (
	"context"
	"database/sql"
	"reflect"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// SetRunCredentialWait changes only the running turn's projection; its capture,
// canonical records and already completed effects remain untouched.
func SetRunCredentialWait(ctx context.Context, db *sql.Conn, node protocol.NodeID, turn protocol.TurnID, waiting *protocol.ConversationRunWaiting) ([]protocol.Event, error) {
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return nil, err
	}
	if run.Status != protocol.ConversationStatusRunning || run.Waiting == nil && waiting == nil {
		return nil, nil
	}
	if waiting != nil {
		same := run.Waiting != nil && run.Waiting.Kind == waiting.Kind && run.Waiting.Credential == waiting.Credential && run.Waiting.Provider == waiting.Provider
		if same && run.Waiting.AccountIdentity != "" && run.Waiting.AuthenticationTarget != "" {
			return nil, nil
		}
		provider, err := capturedRunProvider(ctx, db, turn)
		if err != nil {
			return nil, err
		}
		identity, target, err := ProviderRecoveryIdentity(ctx, db, node, provider, waiting.Credential)
		if err != nil {
			return nil, err
		}
		if same {
			// An older runtime projection may acquire trusted metadata after
			// this wait began. Enrich only that original credential; never
			// overwrite an identity already captured by the waiting step.
			current := *run.Waiting
			if current.AccountIdentity != "" && current.AccountIdentity != identity ||
				current.AuthenticationTarget != "" && current.AuthenticationTarget != target {
				return nil, nil
			}
			if current.AccountIdentity == "" {
				current.AccountIdentity = identity
			}
			if current.AuthenticationTarget == "" {
				current.AuthenticationTarget = target
			}
			if reflect.DeepEqual(run.Waiting, &current) {
				return nil, nil
			}
			waiting = &current
		} else {
			waiting.Attempt = protocol.NewRequestID()
			waiting.AccountIdentity, waiting.AuthenticationTarget = identity, target
		}
	}
	run.Waiting = waiting
	if err := putAgentRun(ctx, db, run); err != nil {
		return nil, err
	}
	return activityEvent(ctx, db, run.Session)
}
