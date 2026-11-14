package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func StartSessionProvider(ctx context.Context, db *sql.Conn, draft protocol.ConversationStart) (protocol.ProviderID, error) {
	config := draft.Config
	if config == nil {
		defaults, err := ReadDefaults(ctx, db)
		if err != nil {
			return protocol.ProviderID{}, err
		}
		config = defaults.Config
	}
	if config == nil {
		return protocol.ProviderID{}, fault(protocol.ErrorCodeNotConfigured, "Node model defaults are not configured")
	}
	effective := *config
	if draft.Execution == protocol.ConversationExecutionTeam {
		var err error
		effective, err = configureSessionTeam(ctx, db, effective, draft.Team)
		if err != nil {
			return protocol.ProviderID{}, err
		}
	}
	return effective.Provider, nil
}

func PluginTransactionProviders(ctx context.Context, db *sql.Conn, request protocol.Request) ([]protocol.ProviderID, error) {
	if request.Plugin == nil {
		return nil, fault(protocol.ErrorCodePermissionDenied, "plugin transaction requires captured provenance")
	}
	input, err := providerPayload[protocol.CommandPluginTransactionPayload](request.Command)
	if err != nil || len(input.Operations) == 0 || len(input.Operations) > 64 {
		return nil, invalid("plugin transaction must contain 1 to 64 operations")
	}
	info, err := ReadPluginVersion(ctx, db, request.Plugin.Package)
	if err != nil {
		return nil, err
	}
	info.Summary.Enabled = true
	providers, seen := []protocol.ProviderID{}, map[protocol.ProviderID]bool{}
	for _, operation := range input.Operations {
		command, err := pluginTransactionCommand(operation, request.Plugin.Package)
		if err != nil {
			return nil, err
		}
		var session protocol.SessionID
		switch value := command.Data.(type) {
		case *protocol.CommandSubmitTurnPayload:
			session = value.Session
		case *protocol.CommandContinueTurnPayload:
			session = value.Session
		default:
			continue
		}
		// Only operations that need a provider are authorized in this read
		// phase. Other operations may depend on earlier writes in the atomic
		// transaction, which checks each command immediately before executing it.
		nested := request
		nested.Command = command
		if err := CheckPluginRequest(ctx, db, nested, info); err != nil {
			return nil, err
		}
		current, err := ReadSession(ctx, db, session)
		if err != nil {
			return nil, err
		}
		if provider := current.Config.Provider; !seen[provider] {
			providers, seen[provider] = append(providers, provider), true
		}
	}
	return providers, nil
}
