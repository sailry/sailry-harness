package store

import (
	"context"
	"database/sql"
	"reflect"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func PluginModels(ctx context.Context, db *sql.Conn) (protocol.PluginModelsCatalog, error) {
	providers, err := providerList(ctx, db)
	result := protocol.PluginModelsCatalog{Models: []protocol.PluginModelsModel{}}
	for _, provider := range providers {
		if !provider.Enabled {
			continue
		}
		for _, model := range provider.Models {
			efforts := []protocol.Effort{}
			if model.Reasoning {
				efforts = slices.Clone(model.Efforts)
			}
			api := provider.Api
			result.Models = append(result.Models, protocol.PluginModelsModel{
				Kind: protocol.PluginModelsKindProvider, ID: provider.ID.String() + "/" + model.ID,
				Provider: provider.Name, Model: model.ID, Api: &api, Authentication: provider.Authentication,
				Default: provider.DefaultModel == model.ID, Reasoning: model.Reasoning, Efforts: efforts, DefaultEffort: model.DefaultEffort,
			})
		}
	}
	return result, err
}

func ResolvePluginModel(ctx context.Context, db *sql.Conn, selected string, effort *protocol.Effort, base *protocol.SessionConfig) (protocol.ConversationProvider, protocol.SessionConfig, error) {
	config := protocol.DefaultSessionConfig()
	if base != nil && (base.Assistant != nil || base.Resource != nil) {
		return protocol.ConversationProvider{}, config, invalid("model selection requires a project or unassigned scope")
	}
	if selected == "" {
		return protocol.ConversationProvider{}, config, fault(protocol.ErrorCodeNotConfigured, "plugin model is not configured")
	}
	providerID, name, found := strings.Cut(selected, "/")
	id, err := protocol.ParseProviderID(providerID)
	if !found || err != nil || name == "" {
		return protocol.ConversationProvider{}, config, invalid("invalid plugin model selection")
	}
	saved, err := currentProvider(ctx, db, id, nil)
	if err != nil {
		return protocol.ConversationProvider{}, config, err
	}
	if !saved.Provider.Enabled {
		return protocol.ConversationProvider{}, config, fault(protocol.ErrorCodeNotConfigured, "selected model provider is unavailable")
	}
	provider := *saved.Provider
	index := slices.IndexFunc(provider.Models, func(value protocol.ConversationModel) bool { return value.ID == name })
	if index < 0 {
		return provider, config, fault(protocol.ErrorCodeNotConfigured, "selected model is unavailable")
	}
	model := provider.Models[index]
	config.Provider, config.Model, config.Credential = provider.ID, name, provider.Credential
	config.Effort = model.DefaultEffort
	if len(model.Efforts) > 0 && !slices.ContainsFunc(model.Efforts, func(value protocol.Effort) bool { return reflect.DeepEqual(value, config.Effort) }) {
		config.Effort = model.Efforts[0]
	}
	if base != nil {
		unchanged := base.Provider == config.Provider && base.Model == config.Model
		selected := config
		config = *base
		config.Provider, config.Model, config.Credential = selected.Provider, selected.Model, selected.Credential
		config.Features = map[string]string{}
		if unchanged {
			for key, value := range base.Features {
				candidate := config
				candidate.Features = map[string]string{key: value}
				if providers.ValidateSelection(provider, candidate) == nil {
					config.Features[key] = value
				}
			}
		} else {
			config.Effort = selected.Effort
		}
	}
	if effort != nil {
		config.Effort = *effort
	}
	err = providers.ValidateSelection(provider, config)
	return provider, config, err
}

func CapturePluginGeneration(ctx context.Context, db *sql.Conn, node protocol.NodeID, selected string, effort *protocol.Effort) (protocol.ConversationProvider, protocol.SessionConfig, *protocol.Secret, error) {
	provider, config, err := ResolvePluginModel(ctx, db, selected, effort, nil)
	if err != nil {
		return provider, config, nil, err
	}
	provider, err = CaptureProvider(ctx, db, provider)
	if err != nil {
		return provider, config, nil, err
	}
	if err := validateConfig(ctx, db, node, config); err != nil {
		return provider, config, nil, err
	}
	var secret *protocol.Secret
	if config.Credential != nil {
		secret, err = resolveProviderKey(ctx, db, node, *config.Credential, provider.ID)
	}
	return provider, config, secret, err
}
