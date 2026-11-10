package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// CaptureSearchProvider freezes public metadata while credentials remain current.
func CaptureSearchProvider(ctx context.Context, db *sql.Conn, selection protocol.SearchModelConfig) (protocol.ConversationProvider, error) {
	saved, err := currentProvider(ctx, db, selection.Provider, nil)
	if err != nil {
		return protocol.ConversationProvider{}, err
	}
	if saved == nil || !saved.Listed || saved.Provider == nil || !saved.Provider.Enabled {
		return protocol.ConversationProvider{}, fault(protocol.ErrorCodeNotConfigured, "search model is unavailable")
	}
	selected := false
	for _, entry := range saved.Provider.Models {
		selected = selected || entry.ID == selection.Model
	}
	if !selected {
		return protocol.ConversationProvider{}, fault(protocol.ErrorCodeNotConfigured, "search model is unavailable")
	}
	return CaptureProvider(ctx, db, *saved.Provider)
}

func ResolveSearchBinding(ctx context.Context, db *sql.Conn, node protocol.NodeID, provider protocol.ConversationProvider, name string, session protocol.SessionID) (model.Binding, error) {
	var secret *protocol.Secret
	var err error
	if provider.Credential != nil {
		secret, err = resolveProviderKey(ctx, db, node, *provider.Credential, provider.ID)
		if err != nil {
			return model.Binding{}, err
		}
	}
	config := protocol.DefaultSessionConfig()
	config.Provider = provider.ID
	config.Model = name
	return model.Capture(provider, config, secret, session)
}
