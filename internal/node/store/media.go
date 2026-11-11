package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadMediaSettings(ctx context.Context, db *sql.Conn) (protocol.MediaSettings, error) {
	saved, err := readBody[protocol.MediaSettings](ctx, db, "SELECT body FROM media_settings WHERE singleton=1")
	if err != nil || saved == nil {
		return protocol.DefaultMediaSettings(), err
	}
	return *saved, nil
}

// MediaModels lists the configured models each media role can be bound to. A
// provider whose channel declaration no longer resolves offers nothing.
func MediaModels(ctx context.Context, db *sql.Conn) ([]protocol.MediaCandidate, error) {
	listed, err := providerList(ctx, db)
	if err != nil {
		return nil, err
	}
	candidates := []protocol.MediaCandidate{}
	for _, provider := range listed {
		if err := fillProviderMetadata(ctx, db, &provider); err != nil {
			return nil, err
		}
		provider, err := CaptureProvider(ctx, db, provider)
		if err != nil {
			continue
		}
		for _, model := range provider.Models {
			kinds := []protocol.MediaKind{}
			for _, kind := range []protocol.MediaKind{protocol.MediaKindVision, protocol.MediaKindImage, protocol.MediaKindVideo} {
				if mediaSupported(kind, provider, model) {
					kinds = append(kinds, kind)
				}
			}
			if len(kinds) > 0 {
				candidates = append(candidates, protocol.MediaCandidate{Provider: provider.ID, ProviderName: provider.Name, Model: model.ID, Kinds: kinds})
			}
		}
	}
	return candidates, nil
}

// mediaSupported reflects the provider's own API and authentication contract;
// a role is never routed to another account or model.
func mediaSupported(kind protocol.MediaKind, provider protocol.ConversationProvider, model protocol.ConversationModel) bool {
	if !provider.Enabled {
		return false
	}
	keyed := provider.Authentication == protocol.AuthenticationApiKey
	switch kind {
	case protocol.MediaKindVision:
		if !model.Vision {
			return false
		}
		if provider.Api == protocol.ConversationModelApiPlugin {
			return provider.Binding != nil && *provider.Binding != nil && (*provider.Binding).Adapter != nil && (*provider.Binding).Adapter.Capabilities.Vision
		}
		switch provider.Authentication {
		case protocol.AuthenticationApiKey, protocol.AuthenticationPlugin:
			return true
		case protocol.AuthenticationChatGpt, protocol.AuthenticationChatGptOAuth:
			return provider.Api == protocol.ConversationModelApiResponses
		case protocol.AuthenticationCopilot:
			return provider.Api == protocol.ConversationModelApiChatCompletions || provider.Api == protocol.ConversationModelApiResponses
		case protocol.AuthenticationClaudeOAuth:
			return provider.Api == protocol.ConversationModelApiAnthropic
		}
		return false
	case protocol.MediaKindImage:
		if keyed {
			return provider.Api == protocol.ConversationModelApiChatCompletions || provider.Api == protocol.ConversationModelApiResponses || provider.Api == protocol.ConversationModelApiGemini
		}
		return (provider.Authentication == protocol.AuthenticationChatGptOAuth || provider.Authentication == protocol.AuthenticationCopilot) && provider.Api == protocol.ConversationModelApiResponses
	case protocol.MediaKindVideo:
		return keyed && (provider.Api == protocol.ConversationModelApiChatCompletions || provider.Api == protocol.ConversationModelApiResponses)
	}
	return false
}

func saveMediaSettings(ctx context.Context, db *sql.Conn, input protocol.MediaSettings) (protocol.Output, []protocol.Event, error) {
	current, err := ReadMediaSettings(ctx, db)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if current.Revision != input.Revision {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeRevisionConflict, "media settings changed")
	}
	for kind, binding := range input.Bindings {
		if binding == nil {
			continue
		}
		saved, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", binding.Provider.String())
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if saved == nil || !saved.Listed || saved.Provider == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "media provider is unavailable")
		}
		provider := *saved.Provider
		if err := fillProviderMetadata(ctx, db, &provider); err != nil {
			return protocol.Output{}, nil, err
		}
		if provider, err = CaptureProvider(ctx, db, provider); err != nil {
			return protocol.Output{}, nil, err
		}
		var model *protocol.ConversationModel
		for index := range provider.Models {
			if provider.Models[index].ID == binding.Model {
				model = &provider.Models[index]
			}
		}
		if model == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "media model is unavailable")
		}
		if !mediaSupported(kind, provider, *model) {
			return protocol.Output{}, nil, invalid("model does not support this media role")
		}
	}
	if input.Revision, err = nextRevision(input.Revision); err != nil {
		return protocol.Output{}, nil, invalid("media settings revision exhausted")
	}
	body, err := encode(input)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO media_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body)
	return protocol.Output{Kind: "media_settings", Data: input}, []protocol.Event{{Kind: "media_settings_changed", Data: input}}, err
}
