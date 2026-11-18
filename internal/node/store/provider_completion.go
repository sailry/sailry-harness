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

func metadataCatalogProvider(endpoint string, authentication protocol.Authentication) (string, error) {
	switch authentication {
	case protocol.AuthenticationChatGpt, protocol.AuthenticationChatGptOAuth:
		return "openai", nil
	case protocol.AuthenticationClaudeOAuth:
		return "anthropic", nil
	case protocol.AuthenticationCopilot:
		return "github-copilot", nil
	}
	switch strings.TrimRight(endpoint, "/") {
	case "https://api.openai.com/v1":
		return "openai", nil
	case "https://api.anthropic.com":
		return "anthropic", nil
	case "https://generativelanguage.googleapis.com/v1beta":
		return "google", nil
	case "https://api.deepseek.com", "https://api.deepseek.com/v1":
		return "deepseek", nil
	}
	return providers.BundledCatalogProvider(endpoint)
}

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

// catalogVision uses an exact channel entry or the agreed origin of the model
// on an unlisted channel. A shared ID alone does not identify the models.
func catalogVision(ctx context.Context, db *sql.Conn, bundled bool, namespace, id string) (bool, error) {
	if namespace == "" {
		entries, err := providerCatalogEntries(ctx, db, protocol.ConversationCatalogQuery{IDs: []string{id}}, bundled)
		if err != nil {
			return false, err
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
				return false, nil
			}
			origin = identity.Canonical
		}
		var valid bool
		namespace, id, valid = strings.Cut(origin, "/")
		if !valid || !boundedCatalogID(namespace, 128) || !boundedCatalogID(id, 256) {
			return false, nil
		}
	}
	entries, err := providerCatalogEntries(ctx, db, protocol.ConversationCatalogQuery{Provider: &namespace, IDs: []string{id}}, bundled)
	if err != nil || len(entries) == 0 {
		return false, err
	}
	reference, err := providers.CatalogModel(entries[0].Body)
	return slices.Contains(reference.Inputs, "image"), err
}

// fillProviderVision resolves absent automatic vision flags on already saved
// models for both controller reads and new turns.
func fillProviderVision(ctx context.Context, db *sql.Conn, provider *protocol.ConversationProvider, modelID string) error {
	var status *protocol.ConversationCatalogStatus
	var namespace string
	for i := range provider.Models {
		entry := &provider.Models[i]
		if modelID != "" && entry.ID != modelID || entry.Vision || entry.CustomCapabilities || entry.Capabilities != nil && entry.Capabilities.Vision != nil {
			continue
		}
		if status == nil {
			current, err := ReadProviderCatalogStatus(ctx, db)
			if err != nil {
				return err
			}
			namespace, err = metadataCatalogProvider(provider.Endpoint, provider.Authentication)
			if err != nil {
				return err
			}
			status = &current
		}
		vision, err := catalogVision(ctx, db, status.Bundled, namespace, entry.ID)
		if err != nil {
			return err
		}
		entry.Vision = vision
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
