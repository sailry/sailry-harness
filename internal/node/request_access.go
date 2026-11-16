package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
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
	var body, captured []byte
	if err := db.QueryRowContext(ctx, "SELECT body,scope FROM requests WHERE caller=?1 AND id=?2", caller[:], id.String()).Scan(&body, &captured); err != nil {
		return outcome, err
	}
	var original protocol.Request
	remoteOutput := false
	if len(captured) > 0 {
		var scope admission.Scope
		if err := json.Unmarshal(captured, &scope); err != nil {
			return outcome, err
		}
		remoteOutput = scope.Remote
		user, err := store.CurrentUser(ctx, db)
		if err != nil {
			return outcome, err
		}
		if scope.User != nil {
			original, err := store.ResolveUser(ctx, db, *scope.User)
			if err != nil || original.ID != user.ID {
				return deniedOutcome(), nil
			}
		} else if !user.Local {
			return deniedOutcome(), nil
		}
		if commandPolicies[scope.Kind] == accessMachine && !user.Administrator {
			return deniedOutcome(), nil
		}
		for _, resource := range scope.Resources {
			if err := authorizeCapturedResource(ctx, db, resource); err != nil {
				return deniedOutcome(), nil
			}
		}
	} else if !json.Valid(body) {
		user, err := store.CurrentUser(ctx, db)
		if err != nil {
			return outcome, err
		}
		if !user.Local {
			return deniedOutcome(), nil
		}
	}
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

func authorizeCapturedResource(ctx context.Context, db *sql.Conn, r admission.Resource) error {
	switch r.Kind {
	case "project":
		id, err := protocol.ParseProjectID(r.ID)
		if err != nil {
			return err
		}
		return store.CheckProjectAccess(ctx, db, id, false)
	case "worktree":
		id, err := protocol.ParseWorktreeID(r.ID)
		if err != nil {
			return err
		}
		return store.CheckWorktreeAccess(ctx, db, id)
	case "session":
		id, err := protocol.ParseSessionID(r.ID)
		if err != nil {
			return err
		}
		return store.CheckSessionAccess(ctx, db, id)
	case "terminal":
		id, err := protocol.ParseTerminalID(r.ID)
		if err != nil {
			return err
		}
		return store.CheckTerminalAccess(ctx, db, id)
	case "turn":
		id, err := protocol.ParseTurnID(r.ID)
		if err != nil {
			return err
		}
		return store.CheckTurnAccess(ctx, db, id)
	case "artifact":
		var id protocol.ArtifactID
		err := id.UnmarshalText([]byte(r.ID))
		if err != nil {
			return err
		}
		return store.CheckArtifactAccess(ctx, db, id)
	case "notification":
		var id protocol.NotificationID
		err := id.UnmarshalText([]byte(r.ID))
		if err != nil {
			return err
		}
		return store.CheckNotificationAccess(ctx, db, id)
	case "approval", "question", "checkpoint", "attachment":
		return store.CheckChildAccess(ctx, db, r.Kind, r.ID)
	}
	return permissionDenied()
}
