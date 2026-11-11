package store

import (
	"context"
	"database/sql"
	"reflect"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func fillProviderMetadata(ctx context.Context, db *sql.Conn, provider *protocol.ConversationProvider) error {
	status, err := ReadProviderCatalogStatus(ctx, db)
	if err != nil {
		return err
	}
	var catalogProvider *string
	switch strings.TrimRight(provider.Endpoint, "/") {
	case "https://api.openai.com/v1":
		value := "openai"
		catalogProvider = &value
	case "https://api.anthropic.com":
		value := "anthropic"
		catalogProvider = &value
	case "https://generativelanguage.googleapis.com/v1beta":
		value := "google"
		catalogProvider = &value
	case "https://api.deepseek.com", "https://api.deepseek.com/v1":
		value := "deepseek"
		catalogProvider = &value
	}
	for i := range provider.Models {
		var reference *protocol.ConversationCatalogModel
		if status.Revision != 0 {
			query := protocol.ConversationCatalogQuery{Provider: catalogProvider, IDs: []string{provider.Models[i].ID}, Limit: 100}
			page, err := readProviderCatalog(ctx, db, query)
			if err != nil {
				return err
			}
			if len(page.Models) > 0 {
				reference = &page.Models[0]
			}
		}
		provider.Models[i] = providers.CompleteModel(provider.Models[i], reference, provider.Api)
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
func refreshProviderMetadata(ctx context.Context, db *sql.Conn) ([]protocol.Event, error) {
	saved, err := readBodies[providerConfiguration](ctx, db, "SELECT body FROM providers ORDER BY rowid")
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
