package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Inspection retains the original request identity and status. When current
// access has changed it returns a completed permission fault, never retries an
// effect or discloses a previously authorized private result.
func (node *Node) inspectOutcomeAccess(ctx context.Context, db *sql.Conn, caller protocol.NodeID, id protocol.RequestID, outcome protocol.RequestOutcome) (protocol.RequestOutcome, error) {
	if outcome.Kind != "completed" {
		return outcome, nil
	}
	var body []byte
	if err := db.QueryRowContext(ctx, "SELECT body FROM requests WHERE caller=?1 AND id=?2", caller[:], id.String()).Scan(&body); err != nil {
		return outcome, err
	}
	var original protocol.Request
	remoteOutput := false
	if json.Valid(body) {
		if err := json.Unmarshal(body, &original); err != nil {
			return outcome, err
		}
		if input, ok := original.Command.Data.(*protocol.CommandCreateSessionAtPayload); ok {
			remoteOutput = input.Target != node.ID()
		}
		if err := node.authorizeCommandDB(ctx, db, caller, original); err != nil {
			return deniedOutcome(), nil
		}
	}
	var output *protocol.Output
	switch value := outcome.Data.(type) {
	case protocol.Result[protocol.Output, protocol.Fault]:
		output = value.Ok
	case *protocol.Result[protocol.Output, protocol.Fault]:
		output = value.Ok
	}
	if output != nil && !remoteOutput {
		if err := authorizeOutcomeValue(ctx, db, *output); err != nil {
			return deniedOutcome(), nil
		}
	}
	return outcome, nil
}
func deniedOutcome() protocol.RequestOutcome {
	failure := permissionDenied().(*protocol.Fault)
	return protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Err: failure}}
}
func authorizeOutcomeValue(ctx context.Context, db *sql.Conn, output protocol.Output) error {
	switch output.Kind {
	// These outputs carry IDs of newly created, still-canonical resources.
	case "session", "terminal", "project", "worktree", "notification", "conversation", "turn", "queued_turn":
		return authorizeResourceValue(ctx, db, reflect.ValueOf(output.Data))
	}
	return nil
}
