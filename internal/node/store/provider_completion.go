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

func metadataCatalogProvider(provider protocol.ConversationProvider) (string, error) {
	switch provider.Authentication {
	case protocol.AuthenticationChatGpt, protocol.AuthenticationChatGptOAuth:
		return "openai", nil
	case protocol.AuthenticationClaudeOAuth:
		return "anthropic", nil
	case protocol.AuthenticationCopilot:
		return "github-copilot", nil
	}
	switch strings.TrimRight(provider.Endpoint, "/") {
	case "https://api.openai.com/v1":
		return "openai", nil
	case "https://api.anthropic.com":
		return "anthropic", nil
	case "https://generativelanguage.googleapis.com/v1beta":
		return "google", nil
	case "https://api.deepseek.com", "https://api.deepseek.com/v1":
		return "deepseek", nil
	}
	return providers.BundledCatalogProvider(provider.Endpoint)
}

func fillProviderMetadata(ctx context.Context, db *sql.Conn, provider *protocol.ConversationProvider) error {
	namespace, err := metadataCatalogProvider(*provider)
	if err != nil {
		return err
	}
	var catalogProvider *string
	if namespace != "" {
		catalogProvider = &namespace
	}
	for i := range provider.Models {
		var reference *protocol.ConversationCatalogModel
		query := protocol.ConversationCatalogQuery{Provider: catalogProvider, IDs: []string{provider.Models[i].ID}, Limit: 100}
		page, err := readProviderCatalog(ctx, db, query)
		if err != nil {
			return err
		}
		if len(page.Models) > 0 {
			reference = &page.Models[0]
		}
		provider.Models[i] = providers.CompleteModel(provider.Models[i], reference, provider.Api)
		if namespace == "" {
			if err := fillAutomaticVision(ctx, db, "", &provider.Models[i]); err != nil {
				return err
			}
		}
		// OpenAI's own Responses channel hosts image generation as a tool of
		// its multimodal chat models. The default applies only there and only
		// until the user chooses the model's tags.
		model := &provider.Models[i]
		official := provider.Authentication == protocol.AuthenticationChatGptOAuth || provider.Authentication == protocol.AuthenticationApiKey && catalogProvider != nil && *catalogProvider == "openai"
		if !model.CustomCapabilities && official && provider.Api == protocol.ConversationModelApiResponses && model.Tools && model.Vision && len(model.Generates) == 0 {
			model.Generates = []protocol.MediaGeneration{protocol.MediaGenerationImageTool}
		}
	}
	return nil
}

// fillAutomaticVision resolves a false automatic flag using facts specific to
// the channel or model. Generic catalog rows can share an ID while referring to
// gateways with different inputs. A declared origin supplies the default for
// an unlisted gateway only when those identities agree.
func fillAutomaticVision(ctx context.Context, db *sql.Conn, namespace string, model *protocol.ConversationModel) error {
	if model.Vision || model.CustomCapabilities || model.Capabilities != nil && model.Capabilities.Vision != nil {
		return nil
	}
	status, err := ReadProviderCatalogStatus(ctx, db)
	if err != nil {
		return err
	}
	id := model.ID
	if namespace == "" {
		entries, err := providerCatalogEntries(ctx, db, protocol.ConversationCatalogQuery{IDs: []string{id}}, status.Bundled)
		if err != nil {
			return err
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
				return nil
			}
			origin = identity.Canonical
		}
		var valid bool
		namespace, id, valid = strings.Cut(origin, "/")
		if !valid || !boundedCatalogID(namespace, 128) || !boundedCatalogID(id, 256) {
			return nil
		}
	}
	entries, err := providerCatalogEntries(ctx, db, protocol.ConversationCatalogQuery{Provider: &namespace, IDs: []string{id}}, status.Bundled)
	if err != nil || len(entries) == 0 {
		return err
	}
	reference, err := providers.CatalogModel(entries[0].Body)
	if err == nil {
		model.Vision = slices.Contains(reference.Inputs, "image")
	}
	return err
}

// captureTurnProvider also resolves automatic vision on older saved channels
// when the selected model is captured for a new turn.
func captureTurnProvider(ctx context.Context, db *sql.Conn, provider protocol.ConversationProvider, model string) (protocol.ConversationProvider, error) {
	captured, err := CaptureProvider(ctx, db, provider)
	if err != nil {
		return captured, err
	}
	for i := range captured.Models {
		entry := &captured.Models[i]
		if entry.ID != model || entry.Vision || entry.CustomCapabilities || entry.Capabilities != nil && entry.Capabilities.Vision != nil {
			continue
		}
		namespace, err := metadataCatalogProvider(captured)
		if err != nil {
			return captured, err
		}
		err = fillAutomaticVision(ctx, db, namespace, entry)
		return captured, err
	}
	return captured, nil
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
