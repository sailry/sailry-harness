package store

import (
	"context"
	"database/sql"
	"reflect"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func SupportsProviderCommand(kind string) bool {
	switch kind {
	case "list_providers", "read_provider_key", "put_provider", "save_provider", "remove_provider", "list_credentials", "list_channel_presets", "refresh_model_catalog", "read_model_catalog", "read_model_scores", "discover_models":
		return true
	}
	return false
}
func providerPayload[T any](command protocol.Command) (T, error) {
	var value T
	err := decodeProviderData(command.Data, &value)
	return value, err
}

func draftProvider(input protocol.ConversationDiscoveryDraft) protocol.ConversationProvider {
	provider := protocol.DefaultConversationProvider()
	provider.ID = input.Provider
	provider.Api = input.Api
	provider.Authentication = protocol.AuthenticationApiKey
	provider.Endpoint = input.Endpoint
	provider.Preset = input.Preset
	provider.Credential = input.Credential
	return provider
}
func currentProvider(ctx context.Context, db *sql.Conn, id protocol.ProviderID, revision *uint64) (*providerConfiguration, error) {
	saved, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", id.String())
	if err != nil {
		return nil, err
	}
	if saved == nil || !saved.Listed || saved.Provider == nil {
		return nil, fault(protocol.ErrorCodeNotFound, "provider does not exist")
	}
	if revision != nil {
		if err := checkRevision(saved.Provider.Revision, *revision); err != nil {
			return nil, err
		}
	}
	return saved, nil
}
func saveProviderConfiguration(ctx context.Context, db *sql.Conn, saved providerConfiguration) error {
	if saved.Quota != nil {
		if err := validateProviderQuota(saved.ID, *saved.Quota); err != nil {
			return err
		}
	}
	body, err := encode(saved)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO providers(id,body) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body", saved.ID.String(), body)
	return err
}
func providerList(ctx context.Context, db *sql.Conn) ([]protocol.ConversationProvider, error) {
	saved, err := readBodies[providerConfiguration](ctx, db, "SELECT body FROM providers ORDER BY rowid")
	if err != nil {
		return nil, err
	}
	values := []protocol.ConversationProvider{}
	for _, item := range saved {
		if item.Listed && item.Provider != nil {
			values = append(values, *item.Provider)
		}
	}
	return values, nil
}
func PrepareProviderCommand(ctx context.Context, db *sql.Conn, node protocol.NodeID, request protocol.Request) (*providers.Work, error) {
	if request.Plugin != nil {
		return nil, fault(protocol.ErrorCodePermissionDenied, "plugin invocation authority is unavailable")
	}
	work := &providers.Work{Command: request.Command}
	switch request.Command.Kind {
	case "refresh_model_catalog":
		state, err := providerScoreState(ctx, db)
		if err != nil {
			return nil, err
		}
		work.ETag = state.ETag
		work.ScoreRevision = state.Status.Revision
		return work, nil
	case "list_channel_presets":
		input, err := providerPayload[protocol.CommandListChannelPresetsPayload](request.Command)
		if err != nil {
			return nil, err
		}
		if !input.RefreshPartners {
			return nil, nil
		}
		return work, nil
	case "discover_models":
		source, err := providerPayload[protocol.ConversationDiscoverySource](request.Command)
		if err != nil {
			return nil, err
		}
		switch source.Kind {
		case "saved":
			var input protocol.ConversationDiscoverySourceSavedPayload
			if err = decodeProviderData(source.Data, &input); err != nil {
				return nil, err
			}
			saved, err := currentProvider(ctx, db, input.Provider, &input.ExpectedRevision)
			if err != nil {
				return nil, err
			}
			work.Provider = saved.Provider
			if input.Oauth != nil {
				provider := *saved.Provider
				provider.Oauth = input.Oauth
				if _, err := providers.OAuthFields(provider); err != nil {
					return nil, err
				}
				work.Provider = &provider
			}
		case "draft":
			var input protocol.ConversationDiscoveryDraft
			if err = decodeProviderData(source.Data, &input); err != nil {
				return nil, err
			}
			provider := draftProvider(input)
			work.Provider = &provider
			work.Secret = input.Secret
		default:
			return nil, invalid("model discovery source is invalid")
		}
	default:
		return nil, nil
	}
	if work.Provider != nil {
		if err := bindProviderPreset(ctx, db, work.Provider, nil, true); err != nil {
			return nil, err
		}
		if work.Secret == nil && work.Provider.Credential != nil {
			key, err := resolveProviderKey(ctx, db, node, *work.Provider.Credential, work.Provider.ID)
			if err != nil {
				return nil, err
			}
			work.Secret = key
		}
	}
	return work, nil
}
func ExecuteProviderCommand(ctx context.Context, db *sql.Conn, node protocol.NodeID, request protocol.Request, prepared *providers.Prepared) (output protocol.Output, changes []protocol.Event, resultErr error) {
	defer func() {
		if resultErr != nil {
			return
		}
		switch request.Command.Kind {
		case "put_provider", "save_provider", "remove_provider":
			var quotaChanges []protocol.Event
			quotaChanges, resultErr = ReconcileProviderQuotas(ctx, db, node)
			changes = append(changes, quotaChanges...)
		}
	}()
	if request.Plugin != nil {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "plugin invocation authority is unavailable")
	}
	command := request.Command
	switch command.Kind {
	case "put_provider", "save_provider":
		var provider protocol.ConversationProvider
		var expected uint64
		var secret *protocol.Secret
		if command.Kind == "save_provider" {
			input, err := providerPayload[protocol.CommandSaveProviderPayload](command)
			if err != nil {
				return protocol.Output{}, nil, err
			}
			provider, expected, secret = input.Provider, input.ExpectedRevision, input.Secret
		} else {
			input, err := providerPayload[protocol.CommandPutProviderPayload](command)
			if err != nil {
				return protocol.Output{}, nil, err
			}
			provider, expected = input.Provider, input.ExpectedRevision
		}
		saved, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", provider.ID.String())
		if err != nil {
			return protocol.Output{}, nil, err
		}
		revision := uint64(0)
		var previous *protocol.ConversationProvider
		if saved != nil && saved.Listed && saved.Provider != nil {
			revision = saved.Provider.Revision
			previous = saved.Provider
		}
		if err = checkRevision(revision, expected); err != nil {
			return protocol.Output{}, nil, err
		}
		if saved == nil {
			saved = &providerConfiguration{ID: provider.ID, Authorizations: map[protocol.CredentialID]providerAuthorization{}}
		}
		if saved.Authorizations == nil {
			saved.Authorizations = map[protocol.CredentialID]providerAuthorization{}
		}
		if secret != nil {
			if provider.Authentication != protocol.AuthenticationApiKey {
				return protocol.Output{}, nil, invalid("stored secrets require API-key authentication")
			}
			if err := validateProviderKey(*secret); err != nil {
				return protocol.Output{}, nil, err
			}
			id := protocol.NewCredentialID()
			saved.Authorizations[id] = providerAuthorization{Authentication: protocol.AuthenticationApiKey, Revision: 1, Key: secret}
			provider.Credential = &protocol.CredentialRef{Node: node, ID: id}
		}
		if err := bindProviderPreset(ctx, db, &provider, previous, false); err != nil {
			return protocol.Output{}, nil, err
		}
		if err := providers.ValidateConfiguration(provider); err != nil {
			return protocol.Output{}, nil, err
		}
		if err := fillProviderMetadata(ctx, db, &provider); err != nil {
			return protocol.Output{}, nil, err
		}
		if err := providers.ValidateConfiguration(provider); err != nil {
			return protocol.Output{}, nil, err
		}
		if provider.Credential != nil {
			if provider.Credential.Node != node {
				return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "credential belongs to another Node")
			}
			authorization, ok := saved.Authorizations[provider.Credential.ID]
			if !ok || authorization.Key == nil {
				return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "provider credential is unavailable")
			}
			if authorization.ExpiresAtMs != nil && *authorization.ExpiresAtMs <= uint64(time.Now().UnixMilli()) {
				return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "credential expired; authenticate again")
			}
			if authorization.Authentication != "" && authorization.Authentication != provider.Authentication {
				return protocol.Output{}, nil, invalid("credential authentication does not match the provider")
			}
			if err := providerCredentialCapacity(ctx, db, provider.ID, 1); err != nil {
				return protocol.Output{}, nil, err
			}
		}
		provider.Revision, err = nextRevision(revision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		endpoint, err := providers.Endpoint(provider.Endpoint)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		provider.Endpoint = strings.TrimRight(endpoint.String(), "/")
		saved.Provider = &provider
		saved.Listed = true
		if err := saveProviderConfiguration(ctx, db, *saved); err != nil {
			return protocol.Output{}, nil, err
		}
		return protocol.Output{Kind: "provider", Data: provider}, []protocol.Event{{Kind: "provider_changed", Data: provider}}, nil
	case "remove_provider":
		input, err := providerPayload[protocol.CommandRemoveProviderPayload](command)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		saved, err := currentProvider(ctx, db, input.Provider, &input.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		saved.Listed = false
		if err := saveProviderConfiguration(ctx, db, *saved); err != nil {
			return protocol.Output{}, nil, err
		}
		values, err := providerList(ctx, db)
		return protocol.Output{Kind: "providers", Data: values}, []protocol.Event{{Kind: "provider_removed", Data: protocol.EventProviderRemovedPayload{ID: input.Provider}}}, err
	case "refresh_model_catalog":
		return applyProviderCatalog(ctx, db, prepared)
	case "discover_models":
		if prepared == nil || prepared.Output == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeInternal, "provider discovery result is missing")
		}
		source, err := providerPayload[protocol.ConversationDiscoverySource](command)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if source.Kind == "draft" {
			var input protocol.ConversationDiscoveryDraft
			if err := decodeProviderData(source.Data, &input); err != nil {
				return protocol.Output{}, nil, err
			}
			draft := draftProvider(input)
			if err := bindProviderPreset(ctx, db, &draft, nil, true); err != nil {
				return protocol.Output{}, nil, err
			}
			if prepared.Provider != nil && !reflect.DeepEqual(draft.Binding, prepared.Provider.Binding) {
				return protocol.Output{}, nil, fault(protocol.ErrorCodeRevisionConflict, "channel declaration changed during model discovery")
			}
			if input.Secret == nil && input.Credential != nil {
				if _, err := resolveProviderKey(ctx, db, node, *input.Credential, input.Provider); err != nil {
					return protocol.Output{}, nil, err
				}
			}
		}
		if source.Kind == "saved" {
			var input protocol.ConversationDiscoverySourceSavedPayload
			if err := decodeProviderData(source.Data, &input); err != nil {
				return protocol.Output{}, nil, err
			}
			saved, err := currentProvider(ctx, db, input.Provider, &input.ExpectedRevision)
			if err != nil {
				return protocol.Output{}, nil, err
			}
			if saved.Provider.Credential != nil {
				if _, err := resolveProviderKey(ctx, db, node, *saved.Provider.Credential, saved.ID); err != nil {
					return protocol.Output{}, nil, err
				}
			}
		}
		return *prepared.Output, nil, nil
	case "list_channel_presets":
		if prepared != nil && prepared.Output != nil {
			var listing protocol.ConversationPresetsListing
			if err := decodeProviderData(prepared.Output.Data, &listing); err != nil {
				return protocol.Output{}, nil, err
			}
			if listing.PartnersUnavailable {
				cached, err := providerCachedPresets(ctx, db)
				if err != nil {
					return protocol.Output{}, nil, err
				}
				listing, err = appendPackagePresets(ctx, db, providers.Listing(cached, true))
				return protocol.Output{Kind: "channel_presets", Data: listing}, nil, err
			}
			catalog := protocol.DefaultConversationPresetsCatalog()
			catalog.Version = 1
			for _, entry := range listing.Entries {
				if entry.Source.Kind == "partner" {
					catalog.Partners = append(catalog.Partners, entry.Preset)
				}
			}
			body, err := encode(catalog)
			if err != nil {
				return protocol.Output{}, nil, err
			}
			if _, err := db.ExecContext(ctx, "INSERT INTO channel_presets(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body); err != nil {
				return protocol.Output{}, nil, err
			}
			listing, err = appendPackagePresets(ctx, db, listing)
			return protocol.Output{Kind: "channel_presets", Data: listing}, nil, err
		}
	}
	output, err := ReadProviderCommand(ctx, db, node, command)
	return output, nil, err
}
func ReadProviderCommand(ctx context.Context, db *sql.Conn, node protocol.NodeID, command protocol.Command) (protocol.Output, error) {
	switch command.Kind {
	case "list_providers":
		values, err := providerList(ctx, db)
		return protocol.Output{Kind: "providers", Data: values}, err
	case "read_provider_key":
		input, err := providerPayload[protocol.CommandReadProviderKeyPayload](command)
		if err != nil {
			return protocol.Output{}, err
		}
		saved, err := currentProvider(ctx, db, input.Provider, &input.ExpectedRevision)
		if err != nil {
			return protocol.Output{}, err
		}
		if saved.Provider.Authentication != protocol.AuthenticationApiKey {
			return protocol.Output{}, fault(protocol.ErrorCodePermissionDenied, "only API keys can be read in provider settings")
		}
		var key *protocol.Secret
		if saved.Provider.Credential != nil {
			key, err = resolveProviderKey(ctx, db, node, *saved.Provider.Credential, saved.ID)
		}
		return protocol.Output{Kind: "provider_key", Data: key}, err
	case "list_credentials":
		values, err := providerCredentials(ctx, db)
		return protocol.Output{Kind: "credentials", Data: values}, err
	case "read_model_catalog":
		input, err := providerPayload[protocol.ConversationCatalogQuery](command)
		if err != nil {
			return protocol.Output{}, err
		}
		page, err := readProviderCatalog(ctx, db, input)
		return protocol.Output{Kind: "model_catalog", Data: page}, err
	case "read_model_scores":
		input, err := providerPayload[[]protocol.ConversationCatalogScoresModelRef](command)
		if err != nil {
			return protocol.Output{}, err
		}
		scores, err := readProviderScores(ctx, db, input)
		return protocol.Output{Kind: "model_scores", Data: scores}, err
	case "list_channel_presets":
		catalog, err := providerCachedPresets(ctx, db)
		if err != nil {
			return protocol.Output{}, err
		}
		listing, err := appendPackagePresets(ctx, db, providers.Listing(catalog, false))
		return protocol.Output{Kind: "channel_presets", Data: listing}, err
	}
	return protocol.Output{}, fault(protocol.ErrorCodeNotConfigured, "provider command is unavailable")
}
func validateProviderKey(secret protocol.Secret) error {
	value := secret.Expose()
	if value == "" || len(value) > 65536 {
		return invalid("credential size must be 1 to 65536 bytes")
	}
	return nil
}
func resolveProviderKey(ctx context.Context, db *sql.Conn, node protocol.NodeID, reference protocol.CredentialRef, provider protocol.ProviderID) (*protocol.Secret, error) {
	if reference.Node != node {
		return nil, fault(protocol.ErrorCodePermissionDenied, "credential belongs to another Node")
	}
	saved, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", provider.String())
	if err != nil {
		return nil, err
	}
	if saved == nil {
		return nil, fault(protocol.ErrorCodeNotConfigured, "provider credential is unavailable")
	}
	authorization, ok := saved.Authorizations[reference.ID]
	if !ok || authorization.Key == nil {
		return nil, fault(protocol.ErrorCodeNotConfigured, "provider credential is unavailable")
	}
	if authorization.ExpiresAtMs != nil && *authorization.ExpiresAtMs <= uint64(time.Now().UnixMilli()) {
		return nil, fault(protocol.ErrorCodePermissionDenied, "credential expired; authenticate again")
	}
	return authorization.Key, nil
}
