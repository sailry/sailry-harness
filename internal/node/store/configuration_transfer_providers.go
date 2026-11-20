package store

import (
	"context"
	"database/sql"
	"errors"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ConfigurationProvider is a provider with its API key. Credential, owner,
// origin and revision are bound to the exporting Node and are left out; the
// ID is kept so that roles, teams, media and sessions still refer to it.
type ConfigurationProvider struct {
	Provider protocol.ConversationProvider `json:"provider"`
	Key      *protocol.Secret              `json:"key,omitempty"`
}

func exportProviders(ctx context.Context, db *sql.Conn, node protocol.NodeID, document *ConfigurationDocument) error {
	saved, err := providerConfigurations(ctx, db)
	if err != nil {
		return err
	}
	for _, item := range saved {
		if !item.Listed || item.Provider == nil {
			continue
		}
		entry := ConfigurationProvider{Provider: *item.Provider}
		if entry.Provider.Authentication == protocol.AuthenticationApiKey && entry.Provider.Credential != nil && entry.Provider.Credential.Node == node {
			// An expired or missing key is exported as no key.
			key, err := providerConfigurationKey(&item, *entry.Provider.Credential)
			var failure *protocol.Fault
			if err != nil && !errors.As(err, &failure) {
				return err
			}
			entry.Key = key
		}
		entry.Provider.Credential, entry.Provider.Owner, entry.Provider.Source, entry.Provider.Shared, entry.Provider.Revision = nil, nil, nil, false, 0
		document.Providers = append(document.Providers, entry)
	}
	slices.SortFunc(document.Providers, func(a, b ConfigurationProvider) int {
		return strings.Compare(a.Provider.ID.String(), b.Provider.ID.String())
	})
	return nil
}

func importProviders(importer *configurationImporter, document ConfigurationDocument) error {
	for _, entry := range document.Providers {
		if err := importer.entry("providers", entry.Provider.Name, func() (protocol.ConfigurationImportAction, *string, []protocol.Event, error) {
			return importer.provider(entry)
		}); err != nil {
			return err
		}
	}
	return nil
}

// provider saves one provider through save_provider, which mints a local
// credential for an imported key. A credential this Node already holds for
// the provider stays selected when it is still valid and the document brings
// no different key; an account sign-in is never carried over.
func (importer *configurationImporter) provider(entry ConfigurationProvider) (protocol.ConfigurationImportAction, *string, []protocol.Event, error) {
	ctx, db := importer.ctx, importer.db
	provider := entry.Provider
	if err := checkProviderWrite(ctx, db, provider.ID); err != nil {
		if isFault(err, protocol.ErrorCodeNotFound) {
			err = fault(protocol.ErrorCodeConflict, "another user on this Node has a provider with this ID")
		}
		return "", nil, nil, err
	}
	saved, err := readProviderConfiguration(ctx, db, provider.ID)
	if err != nil {
		return "", nil, nil, err
	}
	existing := saved != nil && saved.Listed && saved.Provider != nil
	expected := uint64(0)
	provider.Credential, provider.Owner, provider.Source, provider.Shared, provider.Revision = nil, nil, nil, false, 0
	var held *protocol.Secret
	if existing {
		expected = saved.Provider.Revision
		if current := saved.Provider.Credential; current != nil && saved.Provider.Authentication == provider.Authentication {
			if key, err := providerConfigurationKey(saved, *current); err == nil {
				provider.Credential, held = current, key
			}
		}
	}
	var secret *protocol.Secret
	var reason *string
	if provider.Authentication != protocol.AuthenticationApiKey {
		if provider.Credential == nil {
			reason = reasonText("sign in on this computer to use this provider")
		}
	} else if entry.Key != nil && (held == nil || held.Expose() != entry.Key.Expose()) {
		secret = entry.Key
	} else if entry.Key == nil && provider.Credential == nil {
		reason = reasonText("the export has no API key for this provider; add one in its settings")
	}
	_, events, err := ExecuteProviderCommand(ctx, db, importer.node, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{
		Provider: provider, ExpectedRevision: expected, Secret: secret,
	}}}, nil)
	return changedAction(existing), reason, events, err
}
