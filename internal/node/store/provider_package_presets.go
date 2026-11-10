package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func accountPresentation(declaration protocol.PluginModelAuthDeclaration) protocol.PluginModelAuthPresentation {
	presentation := protocol.PluginModelAuthPresentation{Kind: declaration.Flow.Kind}
	switch declaration.Flow.Kind {
	case "script":
		var flow protocol.PluginModelAuthFlowScriptPayload
		body, _ := json.Marshal(declaration.Flow.Data)
		_ = json.Unmarshal(body, &flow)
		presentation.Data = protocol.PluginModelAuthPresentationScriptPayload{Callback: flow.Callback}
	case "pkce":
		var flow protocol.PluginModelAuthFlowPkcePayload
		body, _ := json.Marshal(declaration.Flow.Data)
		_ = json.Unmarshal(body, &flow)
		presentation.Data = protocol.PluginModelAuthPresentationPkcePayload{Redirect: flow.Redirect}
	}
	return presentation
}

func appendPackagePresets(ctx context.Context, db *sql.Conn, listing protocol.ConversationPresetsListing) (protocol.ConversationPresetsListing, error) {
	infos, err := ReadPluginInfos(ctx, db)
	if err != nil {
		return listing, err
	}
	for _, info := range infos {
		if !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" || info.Extension == nil {
			continue
		}
		for index := range listing.Entries {
			preset := listing.Entries[index].Preset
			if preset.Auth.Kind != "builtin" {
				continue
			}
			var auth protocol.ConversationPresetsAuthBuiltinPayload
			body, _ := json.Marshal(preset.Auth.Data)
			_ = json.Unmarshal(body, &auth)
			for _, declaration := range info.Extension.ModelAuth {
				if declaration.Authentication != nil && *declaration.Authentication == auth.Authentication {
					presentation := accountPresentation(declaration)
					listing.Entries[index].Login = &presentation
				}
			}
		}
		if !slices.Contains(info.Extension.Actions, protocol.PluginActionProvideModels) {
			continue
		}
		for _, preset := range info.Extension.ModelProviders {
			reference, origin := plugins.PackageReference(info), pluginAccountOrigin(info)
			entry := protocol.ConversationPresetsEntry{
				Source: protocol.ConversationPresetsSource{Kind: "package", Data: protocol.ConversationPresetsSourcePackagePayload{Name: info.Summary.Name}},
				Preset: preset, Provenance: protocol.ConversationPresetsProvenanceThirdParty, Package: &reference, Origin: &origin,
				Requested: protocol.ConversationConsentScopes{Messages: preset.Adapter != nil}, Icon: info.Extension.Icon,
			}
			if info.Origin != nil && info.Origin.Kind == "bundled" {
				entry.Provenance = protocol.ConversationPresetsProvenance("bundled")
			}
			if preset.Auth.Kind == "plugin" {
				var auth protocol.ConversationPresetsAuthPluginPayload
				body, _ := json.Marshal(preset.Auth.Data)
				_ = json.Unmarshal(body, &auth)
				for _, declaration := range info.Extension.ModelAuth {
					if declaration.ID == auth.Flow {
						presentation := accountPresentation(declaration)
						entry.Login, entry.Requested.Credentials = &presentation, declaration.Exchange != nil
					}
				}
			}
			listing.Entries = append(listing.Entries, entry)
		}
	}
	return listing, nil
}

func bindPackageProvider(ctx context.Context, db *sql.Conn, provider *protocol.ConversationProvider, previous *protocol.ConversationProvider, capture bool) error {
	var source protocol.ConversationPresetsSourcePackagePayload
	body, err := json.Marshal(provider.Preset.Source.Data)
	if err != nil || json.Unmarshal(body, &source) != nil {
		return invalid("channel package source is invalid")
	}
	if capture && provider.Binding != nil && *provider.Binding != nil {
		binding := **provider.Binding
		if binding.Package == nil || binding.Package.Name != source.Name || binding.Origin == nil {
			return invalid("channel package does not match its capture")
		}
		_, err := ReadPluginVersion(ctx, db, *binding.Package)
		return err
	}
	info, err := ReadPlugin(ctx, db, source.Name)
	if err != nil {
		return err
	}
	if info == nil || !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" || info.Extension == nil || !slices.Contains(info.Extension.Actions, protocol.PluginActionProvideModels) {
		if previous != nil && reflect.DeepEqual(previous.Preset, provider.Preset) && reflect.DeepEqual(previous.Binding, provider.Binding) {
			return nil
		}
		return fault(protocol.ErrorCodeNotConfigured, "channel package is unavailable")
	}
	var preset *protocol.ConversationPresetsPreset
	for index := range info.Extension.ModelProviders {
		if info.Extension.ModelProviders[index].ID == provider.Preset.ID {
			preset = &info.Extension.ModelProviders[index]
		}
	}
	if preset == nil {
		return fault(protocol.ErrorCodeNotConfigured, "channel package preset is unavailable")
	}
	matched := false
	for _, route := range preset.Routes {
		key := protocol.ConversationPresetsRouteKey{Protocol: route.Protocol, Region: route.Region, Product: route.Product}
		if providers.RouteAPI(route.Protocol) == provider.Api && (provider.Preset.Route == nil || reflect.DeepEqual(*provider.Preset.Route, key)) {
			matched = true
		}
	}
	if !matched {
		return invalid("channel package route does not match the provider")
	}
	binding := protocol.DefaultConversationPresetsBinding()
	reference, origin := plugins.PackageReference(*info), pluginAccountOrigin(*info)
	binding.Package, binding.Origin, binding.Policies = &reference, &origin, slices.Clone(preset.Policies)
	authentication := protocol.AuthenticationApiKey
	switch preset.Auth.Kind {
	case "plugin":
		var auth protocol.ConversationPresetsAuthPluginPayload
		body, _ := json.Marshal(preset.Auth.Data)
		if json.Unmarshal(body, &auth) != nil {
			return invalid("channel package authentication is invalid")
		}
		binding.Auth, authentication = &protocol.PluginModelAuthBinding{Flow: auth.Flow}, protocol.AuthenticationPlugin
	case "builtin":
		var auth protocol.ConversationPresetsAuthBuiltinPayload
		body, _ := json.Marshal(preset.Auth.Data)
		if json.Unmarshal(body, &auth) != nil {
			return invalid("channel package authentication is invalid")
		}
		authentication = auth.Authentication
	case "api_key":
		var auth protocol.ConversationPresetsAuthApiKeyPayload
		body, _ := json.Marshal(preset.Auth.Data)
		if json.Unmarshal(body, &auth) != nil {
			return invalid("channel package authentication is invalid")
		}
		if auth.Header != nil || auth.Prefix != nil {
			injection := protocol.PluginModelAuthInjection{Header: "authorization", Template: "{key}"}
			if auth.Header != nil {
				injection.Header = *auth.Header
			}
			if auth.Prefix != nil {
				injection.Template = *auth.Prefix + "{key}"
			}
			binding.KeyInjection = &injection
		}
	case "none":
	default:
		return invalid("channel package authentication is unsupported")
	}
	if provider.Authentication != authentication {
		return invalid("channel authentication does not match the package preset")
	}
	if preset.Adapter != nil {
		for _, adapter := range info.Extension.ModelAdapters {
			if adapter.ID == *preset.Adapter {
				binding.Adapter = &protocol.PluginModelAdapterBinding{Adapter: adapter.ID, Capabilities: adapter.Capabilities}
			}
		}
		if binding.Adapter == nil {
			return fault(protocol.ErrorCodeNotConfigured, "channel package adapter is unavailable")
		}
	}
	pointer := &binding
	provider.Binding = &pointer
	return nil
}
