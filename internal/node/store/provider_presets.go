package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func providerCachedPresets(ctx context.Context, db *sql.Conn) (protocol.ConversationPresetsCatalog, error) {
	var raw []byte
	err := db.QueryRowContext(ctx, "SELECT body FROM channel_presets WHERE singleton=1").Scan(&raw)
	if err == sql.ErrNoRows {
		return protocol.DefaultConversationPresetsCatalog(), nil
	}
	if err != nil {
		return protocol.ConversationPresetsCatalog{}, err
	}
	catalog, err := providers.ParsePresets(raw)
	if err != nil {
		return protocol.DefaultConversationPresetsCatalog(), nil
	}
	return catalog, nil
}
func bindProviderPreset(ctx context.Context, db *sql.Conn, provider *protocol.ConversationProvider, previous *protocol.ConversationProvider, capture bool) error {
	if !capture {
		if err := providers.ValidateBinding(*provider); err != nil {
			return err
		}
	}
	if provider.Preset == nil {
		return nil
	}
	reference := provider.Preset
	if reference.Source.Kind == "package" {
		return bindPackageProvider(ctx, db, provider, previous, capture)
	}
	catalog := providers.Bundled()
	presets := catalog.Builtin
	if reference.Source.Kind == "partner" {
		cached, err := providerCachedPresets(ctx, db)
		if err != nil {
			return err
		}
		presets = cached.Partners
		if provider.Authentication != protocol.AuthenticationApiKey || provider.Api == protocol.ConversationModelApiPlugin {
			return invalid("partner channels require a native key-based protocol")
		}
	}
	var selected *protocol.ConversationPresetsPreset
	for i := range presets {
		if presets[i].ID == reference.ID {
			selected = &presets[i]
			break
		}
	}
	if selected == nil {
		if reference.Source.Kind == "partner" && previous != nil && reflect.DeepEqual(previous.Preset, provider.Preset) {
			provider.Binding = previous.Binding
			return nil
		}
		// Captured partner declarations continue independently of a changing suggestion feed.
		if capture && reference.Source.Kind == "partner" && provider.Binding != nil {
			return providers.ValidateBinding(*provider)
		}
		return fault(protocol.ErrorCodeNotConfigured, "channel preset is unavailable on this Node")
	}
	matched := false
	for _, route := range selected.Routes {
		if providers.RouteAPI(route.Protocol) != provider.Api {
			continue
		}
		key := protocol.ConversationPresetsRouteKey{Protocol: route.Protocol, Region: route.Region, Product: route.Product}
		if reference.Route == nil || reflect.DeepEqual(*reference.Route, key) {
			matched = true
			break
		}
	}
	if !matched {
		return invalid("channel preset route does not match the selected protocol")
	}
	authentication := protocol.AuthenticationApiKey
	if selected.Auth.Kind == "builtin" {
		var auth protocol.ConversationPresetsAuthBuiltinPayload
		raw, _ := json.Marshal(selected.Auth.Data)
		if err := json.Unmarshal(raw, &auth); err != nil {
			return err
		}
		authentication = auth.Authentication
	}
	if provider.Authentication != authentication {
		return invalid("channel authentication does not match its preset")
	}
	if reference.Source.Kind == "partner" || capture {
		if capture && provider.Binding != nil && *provider.Binding != nil {
			return nil
		}
		binding := protocol.DefaultConversationPresetsBinding()
		if provider.Authentication == protocol.AuthenticationApiKey && provider.Api == protocol.ConversationModelApiChatCompletions {
			binding.Policies = catalog.Policies
		}
		binding.Policies = append(binding.Policies, selected.Policies...)
		if selected.Auth.Kind == "api_key" {
			var auth protocol.ConversationPresetsAuthApiKeyPayload
			raw, _ := json.Marshal(selected.Auth.Data)
			if err := json.Unmarshal(raw, &auth); err != nil {
				return err
			}
			if auth.Header != nil || auth.Prefix != nil {
				name, prefix := "authorization", ""
				if auth.Header != nil {
					name = *auth.Header
				}
				if auth.Prefix != nil {
					prefix = *auth.Prefix
				}
				binding.KeyInjection = &protocol.PluginModelAuthInjection{Header: name, Template: prefix + "{key}"}
			}
		}
		pointer := &binding
		provider.Binding = &pointer
	}
	return nil
}

// CaptureProvider freezes current native declaration policy independently of the
// saved configuration. Inherited turns retain their existing captured binding.
func CaptureProvider(ctx context.Context, db *sql.Conn, provider protocol.ConversationProvider) (protocol.ConversationProvider, error) {
	raw, err := encode(provider)
	if err != nil {
		return protocol.ConversationProvider{}, err
	}
	var captured protocol.ConversationProvider
	if err := json.Unmarshal(raw, &captured); err != nil {
		return captured, err
	}
	if err := bindProviderPreset(ctx, db, &captured, nil, true); err != nil {
		return captured, err
	}
	return captured, nil
}
