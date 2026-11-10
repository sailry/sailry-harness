package store

import (
	"context"
	"database/sql"
	"reflect"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type ProviderResourceCapture struct {
	Provider   protocol.ConversationProvider
	Endpoint   protocol.PluginModelAuthEndpoint
	Secret     protocol.Secret
	Credential protocol.Credential
}

func CaptureProviderResource(ctx context.Context, db *sql.Conn, node protocol.NodeID, caller protocol.PluginReference, id protocol.ProviderID, revision uint64, name string, mutation bool) (ProviderResourceCapture, error) {
	var capture ProviderResourceCapture
	saved, err := currentProvider(ctx, db, id, &revision)
	if err != nil {
		return capture, err
	}
	provider, err := CaptureProvider(ctx, db, *saved.Provider)
	if err != nil {
		return capture, err
	}
	if !provider.Enabled || provider.Credential == nil {
		return capture, fault(protocol.ErrorCodeNotConfigured, "provider authorization is unavailable")
	}
	info, err := ReadPlugin(ctx, db, caller.Name)
	if err != nil {
		return capture, err
	}
	if info == nil || !info.Summary.Enabled || plugins.PackageReference(*info) != caller || info.Extension == nil {
		return capture, fault(protocol.ErrorCodePermissionDenied, "provider resource does not match the calling plugin")
	}
	var endpoint protocol.PluginModelAuthEndpoint
	found := false
	if provider.Authentication == protocol.AuthenticationPlugin {
		if provider.Binding == nil || *provider.Binding == nil || (**provider.Binding).Package == nil || *(**provider.Binding).Package != caller {
			return capture, fault(protocol.ErrorCodePermissionDenied, "provider resource does not match its captured package")
		}
		_, declaration, err := providerAuthDeclaration(ctx, db, provider)
		if err != nil {
			return capture, err
		}
		requests := declaration.Requests
		if mutation {
			requests = declaration.Mutations
		}
		endpoint, found = requests[name]
	} else {
		if mutation || provider.Authentication == protocol.AuthenticationApiKey {
			return capture, fault(protocol.ErrorCodePermissionDenied, "native provider resources permit declared account reads only")
		}
		for _, resource := range info.Extension.ModelResources {
			if resource.Authentication == provider.Authentication {
				endpoint, found = resource.Requests[name]
			}
		}
		if found && !providers.AccountResourceEndpoint(provider.Authentication, endpoint.Url) {
			return capture, fault(protocol.ErrorCodePermissionDenied, "provider resource origin is not authorized")
		}
	}
	if !found {
		return capture, fault(protocol.ErrorCodeNotConfigured, "provider resource is not declared")
	}
	secret, err := resolveProviderKey(ctx, db, node, *provider.Credential, provider.ID)
	if err != nil {
		return capture, err
	}
	capture = ProviderResourceCapture{Provider: provider, Endpoint: endpoint, Secret: *secret,
		Credential: providerCredentialMetadata(provider.ID, provider.Credential.ID, saved.Authorizations[provider.Credential.ID])}
	return capture, nil
}

func CheckProviderResource(ctx context.Context, db *sql.Conn, capture ProviderResourceCapture) error {
	saved, err := currentProvider(ctx, db, capture.Provider.ID, &capture.Provider.Revision)
	if err != nil {
		return err
	}
	provider, err := CaptureProvider(ctx, db, *saved.Provider)
	if err != nil {
		return err
	}
	if !reflect.DeepEqual(provider, capture.Provider) {
		return fault(protocol.ErrorCodeRevisionConflict, "provider resource configuration changed")
	}
	current := providerCredentialMetadata(provider.ID, capture.Credential.ID, saved.Authorizations[capture.Credential.ID])
	if current.Revision != capture.Credential.Revision || current.Revoked {
		return fault(protocol.ErrorCodeRevisionConflict, "provider resource authorization changed")
	}
	return nil
}
