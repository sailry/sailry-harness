package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func CaptureCacheToolsBinding(ctx context.Context, db *sql.Conn, node protocol.NodeID, config protocol.SessionConfig) (model.Binding, error) {
	saved, err := currentProvider(ctx, db, config.Provider, nil)
	if err != nil {
		return model.Binding{}, errors.New("The selected cache-tools channel is unavailable")
	}
	provider, err := CaptureProvider(ctx, db, *saved.Provider)
	if err != nil {
		return model.Binding{}, errors.New("Could not capture the selected channel configuration")
	}
	if provider.Api != protocol.ConversationModelApiResponses || provider.Authentication != protocol.AuthenticationApiKey || model.PluginBound(provider) {
		return model.Binding{}, errors.New("cache-tools requires a native Responses channel with API-key authentication")
	}
	if config.Credential == nil {
		config.Credential = provider.Credential
	}
	if err := validateConfig(ctx, db, node, config); err != nil {
		return model.Binding{}, errors.New("The selected model, effort, or credential is unavailable")
	}
	var secret *protocol.Secret
	if config.Credential != nil {
		secret, err = resolveCapturedProviderKey(ctx, db, node, *config.Credential, provider)
		if err != nil {
			return model.Binding{}, errors.New("Could not resolve the selected channel credential")
		}
	}
	binding, err := model.Capture(provider, config, secret, protocol.SessionID{})
	if err != nil {
		return model.Binding{}, errors.New("Could not bind the selected Responses model")
	}
	return binding, nil
}
