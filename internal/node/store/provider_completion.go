package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func fillProviderMetadata(ctx context.Context, db *sql.Conn, provider *protocol.ConversationProvider) error {
	channel := &protocol.ConversationCatalogChannel{Endpoint: provider.Endpoint, Authentication: provider.Authentication}
	for i := range provider.Models {
		var reference *protocol.ConversationCatalogModel
		query := protocol.ConversationCatalogQuery{Channel: channel, IDs: []string{provider.Models[i].ID}, Limit: 1}
		page, err := readProviderCatalog(ctx, db, query)
		if err != nil {
			return err
		}
		if len(page.Models) > 0 {
			reference = &page.Models[0]
		}
		provider.Models[i] = providers.CompleteChannelModel(*provider, provider.Models[i], reference)
	}
	return nil
}

// catalogOffering finds the exact offering, or the agreed canonical origin
// when no namespace is specified. The latter is used only for legacy vision.
func catalogOffering(ctx context.Context, db *sql.Conn, bundled bool, namespace, id string) (*protocol.ConversationCatalogModel, error) {
	if namespace == "" {
		entries, err := providerCatalogEntries(ctx, db, protocol.ConversationCatalogQuery{IDs: []string{id}}, bundled)
		if err != nil {
			return nil, err
		}
		origin := ""
		for _, entry := range entries {
			var identity struct {
				Canonical string `json:"canonical_model_id"`
			}
			if json.Unmarshal(entry.Body, &identity) != nil || identity.Canonical == "" {
				continue
			}
			if origin != "" && origin != identity.Canonical {
				return nil, nil
			}
			origin = identity.Canonical
		}
		var valid bool
		namespace, id, valid = strings.Cut(origin, "/")
		if !valid || !boundedCatalogID(namespace, 128) || !boundedCatalogID(id, 256) {
			return nil, nil
		}
	}
	entries, err := providerCatalogEntries(ctx, db, protocol.ConversationCatalogQuery{Provider: &namespace, IDs: []string{id}}, bundled)
	if err != nil || len(entries) == 0 {
		return nil, err
	}
	reference, err := providers.CatalogModel(entries[0].Body)
	if err != nil || reference.ID != id {
		return nil, err
	}
	return &reference, nil
}

// fillProviderVision resolves input capabilities on already saved legacy models
// for controller reads and new turns without revising the configuration.
// Controller lists also backfill native outputs for existing configurations.
func fillProviderVision(ctx context.Context, db *sql.Conn, provider *protocol.ConversationProvider, modelID string) error {
	var status *protocol.ConversationCatalogStatus
	var namespace string
	for i := range provider.Models {
		entry := &provider.Models[i]
		if modelID != "" && entry.ID != modelID {
			continue
		}
		fillInputs := entry.Inputs == nil && !entry.CustomCapabilities
		fillOutputs := modelID == "" && len(entry.Outputs) == 0
		if !fillInputs && !fillOutputs {
			continue
		}
		if status == nil {
			current, err := ReadProviderCatalogStatus(ctx, db)
			if err != nil {
				return err
			}
			namespace, err = providers.ChannelCatalogNamespace(provider.Endpoint, provider.Authentication)
			if err != nil {
				return err
			}
			status = &current
		}
		var inputs []string
		if namespace != "" || fillInputs && !entry.Vision && (entry.Capabilities == nil || entry.Capabilities.Vision == nil) {
			offering, err := catalogOffering(ctx, db, status.Bundled, namespace, entry.ID)
			if err != nil {
				return err
			}
			if offering != nil {
				if namespace != "" {
					inputs = offering.Inputs
					if fillOutputs {
						entry.Outputs = providers.CatalogOutputs(offering.Outputs)
					}
				} else {
					entry.Vision = slices.Contains(offering.Inputs, "image")
				}
			}
		}
		if fillInputs {
			*entry = providers.CompleteModelInputs(*entry, inputs)
		}
	}
	return nil
}

func captureTurnProvider(ctx context.Context, db *sql.Conn, provider protocol.ConversationProvider, model string) (protocol.ConversationProvider, error) {
	captured, err := CaptureProvider(ctx, db, provider)
	if err == nil {
		err = fillProviderVision(ctx, db, &captured, model)
	}
	return captured, err
}

func refreshProviderMetadata(ctx context.Context, db *sql.Conn) ([]protocol.Event, error) {
	saved, err := providerConfigurations(ctx, db)
	if err != nil {
		return nil, err
	}
	events := []protocol.Event{}
	for _, stored := range saved {
		if !stored.Listed || stored.Provider == nil {
			continue
		}
		provider := *stored.Provider
		provider.Models = append([]protocol.ConversationModel(nil), provider.Models...)
		if err := fillProviderMetadata(ctx, db, &provider); err != nil {
			return nil, err
		}
		if reflect.DeepEqual(provider, *stored.Provider) {
			continue
		}
		if err := providers.ValidateConfiguration(provider); err != nil {
			return nil, err
		}
		provider.Revision, err = nextRevision(provider.Revision)
		if err != nil {
			return nil, err
		}
		stored.Provider = &provider
		if err := saveProviderConfiguration(ctx, db, stored); err != nil {
			return nil, err
		}
		events = append(events, protocol.Event{Kind: "provider_changed", Data: provider})
	}
	return events, nil
}
