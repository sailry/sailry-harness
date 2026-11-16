package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func identityAction(command protocol.Command) (protocol.IdentityCommand, error) {
	if command.Kind != "identity" {
		return protocol.IdentityCommand{}, invalid("unsupported identity command")
	}
	switch value := command.Data.(type) {
	case protocol.IdentityCommand:
		return value, nil
	case *protocol.IdentityCommand:
		if value != nil {
			return *value, nil
		}
	}
	return protocol.IdentityCommand{}, invalid("invalid identity command")
}

func ReadIdentityCommand(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, command protocol.Command) (protocol.Output, error) {
	action, err := identityAction(command)
	if err != nil {
		return protocol.Output{}, err
	}
	switch action.Kind {
	case "read_identity":
		value, err := ReadIdentity(ctx, db, node, caller)
		return protocol.Output{Kind: "identity_state", Data: value}, err
	case "read_host_directory":
		value, err := ReadIdentityDirectory(ctx, db, node, caller)
		return protocol.Output{Kind: "host_directory", Data: value}, err
	}
	return protocol.Output{}, invalid("unsupported identity read")
}

func ExecuteIdentityCommand(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, error) {
	action, err := identityAction(request.Command)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	switch action.Kind {
	case "sponsor_device":
		value, err := identityPayload[protocol.SponsorDevice](action.Data)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		result, err := SponsorIdentityDevice(ctx, db, node, caller, request.ID, value)
		return protocol.Output{Kind: "device_sponsorship", Data: result}, nil, err
	case "confirm_sponsorship":
		value, err := identityPayload[protocol.ConfirmSponsorship](action.Data)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		result, err := ConfirmIdentitySponsorship(ctx, db, node, caller, value)
		return protocol.Output{Kind: "device_sponsorship", Data: result}, nil, err
	case "revoke_device":
		value, err := identityPayload[protocol.RevokeDevice](action.Data)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		err = RevokeIdentityDevice(ctx, db, node, caller, value)
		return protocol.Output{Kind: "device_revoked", Data: protocol.DeviceRevoked{Device: value.Device}}, nil, err
	case "set_user_administrator":
		value, err := identityPayload[protocol.UserAdministrator](action.Data)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		result, err := SetIdentityAdministrator(ctx, db, node, caller, value)
		return protocol.Output{Kind: "user_changed", Data: result}, nil, err
	case "assign_orphan_user":
		value, err := identityPayload[protocol.AssignOrphanUser](action.Data)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		result, err := AssignIdentityOrphan(ctx, db, node, caller, value)
		return protocol.Output{Kind: "user_changed", Data: result}, nil, err
	}
	return protocol.Output{}, nil, invalid("unsupported identity mutation")
}

func identityPayload[T any](value any) (T, error) {
	switch data := value.(type) {
	case T:
		return data, nil
	case *T:
		if data != nil {
			return *data, nil
		}
	}
	var zero T
	return zero, invalid("invalid identity payload")
}
