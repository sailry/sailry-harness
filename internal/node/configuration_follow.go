package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func SupportsConfigurationFollow(kind string) bool { return kind == "configuration_follow" }

func (node *Node) ReadConfigurationFollow(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	input, ok := dispatchPayload[protocol.ConfigurationFollowCommand](request.Command.Data)
	if !ok {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "configuration follow command is invalid"}
	}
	if input.Action != "read" {
		return protocol.Output{}, retiredConfigurationFollow()
	}
	var output protocol.Output
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		value, err := store.ReadConfigurationFollow(ctx, db)
		output = protocol.Output{Kind: "configuration_follow", Data: value}
		return err
	})
	return output, err
}

func retiredConfigurationFollow() error {
	return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "configuration is managed on its owning Node"}
}

// Retired write actions reach the normal ledger so an already settled request
// can still return its original outcome.
func ExecuteConfigurationFollow(command protocol.Command) (protocol.Output, []protocol.Event, error) {
	input, ok := dispatchPayload[protocol.ConfigurationFollowCommand](command.Data)
	if ok {
		switch input.Action {
		case "apply", "sync", "kv_ack", "kv_snapshot":
			return protocol.Output{}, nil, retiredConfigurationFollow()
		}
	}
	return protocol.Output{}, nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "configuration follow action is not supported"}
}
