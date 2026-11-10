package providers

import (
	"encoding/json"
	"reflect"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ValidateConfiguration(provider protocol.ConversationProvider) error {
	raw, err := json.Marshal(provider)
	if err != nil {
		return invalid("provider configuration is invalid")
	}
	if len(raw) > 64*1024 {
		return invalid("provider configuration exceeds its size limit")
	}
	if strings.TrimSpace(provider.Name) == "" || len(provider.Name) > 128 || len(provider.Endpoint) > 2048 {
		return invalid("provider name and endpoint are required and must be bounded")
	}
	if _, err := Endpoint(provider.Endpoint); err != nil {
		return err
	}
	ids := map[string]bool{}
	for _, model := range provider.Models {
		if strings.TrimSpace(model.ID) == "" || len(model.ID) > 256 || ids[model.ID] || model.Context == 0 || model.Output == 0 || model.Output > model.Context || model.Output > 2147483647 || model.CompactionTokens != nil && *model.CompactionTokens == 0 {
			return invalid("model configuration is invalid or unsupported")
		}
		ids[model.ID] = true
		if model.Reasoning {
			if len(model.Efforts) > 16 || len(model.Efforts) == 0 && model.DefaultEffort.Kind != "default" {
				return invalid("reasoning choices must be unique, bounded and include the default")
			}
			hasDefault := len(model.Efforts) == 0
			for i, effort := range model.Efforts {
				if reflect.DeepEqual(effort, model.DefaultEffort) {
					hasDefault = true
				}
				for _, previous := range model.Efforts[:i] {
					if reflect.DeepEqual(previous, effort) {
						return invalid("reasoning choices must be unique, bounded and include the default")
					}
				}
				if err := validateEffort(provider.Api, effort, model.Output); err != nil {
					return err
				}
			}
			if !hasDefault {
				return invalid("reasoning choices must be unique, bounded and include the default")
			}
		}
		if err := ValidateFeatures(provider.Api, model, nil); err != nil {
			return err
		}
	}
	if len(provider.Models) > 0 && !ids[provider.DefaultModel] {
		return invalid("default model is not in this provider")
	}
	if _, err := OAuthFields(provider); err != nil {
		return err
	}
	if provider.Api == protocol.ConversationModelApiPlugin && (provider.Binding == nil || *provider.Binding == nil || (**provider.Binding).Adapter == nil) {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "plugin model requires its captured translation"}
	}
	if err := ValidateBinding(provider); err != nil {
		return err
	}
	if provider.Api == protocol.ConversationModelApiAzureOpenAi {
		if provider.Options == nil || provider.Options.Kind != "azure_open_ai" {
			return invalid("connection options do not match the provider")
		}
		var options protocol.ConversationConnectionOptionsAzureOpenAiPayload
		if unionData(provider.Options.Data, &options) != nil || !cloudSegment(options.ApiVersion) {
			return invalid("connection options do not match the provider")
		}
		for _, model := range provider.Models {
			if !cloudSegment(model.ID) {
				return invalid("connection options do not match the provider")
			}
		}
	} else if provider.Options != nil {
		if provider.Options.Kind != "hosted" || (provider.Api != protocol.ConversationModelApiChatCompletions && provider.Api != protocol.ConversationModelApiAnthropic && provider.Api != protocol.ConversationModelApiDeepSeek) {
			return invalid("connection options do not match the provider")
		}
	}
	return nil
}
func ValidateSelection(provider protocol.ConversationProvider, config protocol.SessionConfig) error {
	for _, model := range provider.Models {
		if model.ID == config.Model {
			if model.Reasoning {
				found := len(model.Efforts) == 0 && config.Effort.Kind == "default"
				for _, effort := range model.Efforts {
					found = found || reflect.DeepEqual(effort, config.Effort)
				}
				if !found {
					return invalid("reasoning choice is not configured for this model")
				}
				if err := validateEffort(provider.Api, config.Effort, model.Output); err != nil {
					return err
				}
			}
			return ValidateFeatures(provider.Api, model, config.Features)
		}
	}
	if len(config.Features) > 0 {
		return invalid("model feature choices require an advertised model")
	}
	return validateEffort(provider.Api, config.Effort, 0)
}
func validateEffort(api protocol.ConversationModelApi, effort protocol.Effort, output uint32) error {
	budget := int32(0)
	if effort.Kind == "budget" {
		switch v := effort.Data.(type) {
		case int32:
			budget = v
		case *int32:
			if v != nil {
				budget = *v
			}
		default:
			return invalid("reasoning choice is unsupported by the API or output limit")
		}
	}
	supported := false
	switch api {
	case protocol.ConversationModelApiPlugin:
		supported = true
	case protocol.ConversationModelApiAzureAi:
		supported = effort.Kind == "default"
	case protocol.ConversationModelApiAzureOpenAi:
		return validateEffort(protocol.ConversationModelApiChatCompletions, effort, output)
	case protocol.ConversationModelApiDeepSeek:
		supported = effort.Kind == "default" || effort.Kind == "none" || effort.Kind == "high" || effort.Kind == "max"
	case protocol.ConversationModelApiOpenCodeGo:
		if effort.Kind == "budget" {
			supported = budget >= 1024 && uint32(budget) < output
		} else {
			supported = true
		}
	case protocol.ConversationModelApiOpenCodeZen:
		if effort.Kind == "budget" {
			supported = budget == -1 || budget > 0
		} else {
			supported = true
		}
	default:
		switch effort.Kind {
		case "default", "low", "medium", "high":
			supported = true
		default:
			switch api {
			case protocol.ConversationModelApiChatCompletions, protocol.ConversationModelApiResponses:
				supported = effort.Kind != "budget"
			case protocol.ConversationModelApiAnthropic:
				supported = effort.Kind == "xhigh" || effort.Kind == "max" || effort.Kind == "budget" && budget >= 1024 && uint32(budget) < output
			case protocol.ConversationModelApiGemini:
				supported = effort.Kind == "none" || effort.Kind == "budget" && budget > 0
			}
		}
	}
	if !supported {
		return invalid("reasoning choice is unsupported by the API or output limit")
	}
	return nil
}
func ValidateFeatures(api protocol.ConversationModelApi, model protocol.ConversationModel, selected map[string]string) error {
	var features []protocol.ConversationFeaturesFeature
	if model.Capabilities != nil {
		features = model.Capabilities.Features
	}
	ids := map[string]bool{}
	advertised := map[string]map[string]bool{}
	if len(features) > 16 {
		return invalid("model feature declarations must be unique and bounded")
	}
	validID := func(value string) bool {
		if value == "" || len(value) > 64 {
			return false
		}
		for _, r := range value {
			if !(r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r == '-' || r == '_') {
				return false
			}
		}
		return true
	}
	for _, feature := range features {
		if !validID(feature.ID) || ids[feature.ID] || len(feature.Options) == 0 || len(feature.Options) > 16 ||
			feature.Name != nil && !identifier(*feature.Name, 256) || feature.Icon != nil && !feature.Icon.Valid() {
			return invalid("model feature declarations must be unique and bounded")
		}
		ids[feature.ID] = true
		options := map[string]bool{}
		for _, option := range feature.Options {
			if !validID(option.ID) || options[option.ID] || option.Name != nil && !identifier(*option.Name, 256) || option.Icon != nil && !option.Icon.Valid() {
				return invalid("model feature declarations must be unique and bounded")
			}
			options[option.ID] = true
		}
		advertised[feature.ID] = options
	}
	for id, value := range selected {
		if !advertised[id][value] {
			return invalid("model feature choice is not advertised")
		}
		if api != protocol.ConversationModelApiPlugin && !(api == protocol.ConversationModelApiResponses && id == "speed" && (value == "fast" || value == "ultrafast")) {
			return invalid("model feature choice is unsupported by the API")
		}
	}
	return nil
}
func ValidateBinding(provider protocol.ConversationProvider) error {
	if provider.Binding == nil || *provider.Binding == nil {
		return nil
	}
	if provider.Preset == nil || (provider.Preset.Source.Kind != "partner" && provider.Preset.Source.Kind != "package") {
		return invalid("captured channel bindings cannot be saved as configuration")
	}
	binding := **provider.Binding
	if provider.Preset.Source.Kind == "partner" && (binding.Package != nil || binding.Origin != nil || binding.Auth != nil || binding.Adapter != nil) {
		return invalid("partner policy cannot bind executable package resources")
	}
	if provider.Preset.Source.Kind == "package" && (binding.Package == nil || binding.Origin == nil) {
		return invalid("package policy requires an immutable package capture")
	}
	for _, policy := range binding.Policies {
		if err := ValidatePolicy(policy); err != nil {
			return err
		}
	}
	if binding.KeyInjection != nil {
		if err := validateInjection(*binding.KeyInjection); err != nil {
			return err
		}
		if provider.Api != protocol.ConversationModelApiChatCompletions && provider.Api != protocol.ConversationModelApiResponses && provider.Api != protocol.ConversationModelApiAnthropic && !(provider.Api == protocol.ConversationModelApiPlugin && binding.Adapter != nil) {
			return invalid("channel protocol does not support custom key headers")
		}
	}
	return nil
}
func cloudSegment(value string) bool {
	if value == "" || len(value) > 128 || value == "." || value == ".." {
		return false
	}
	for _, r := range value {
		if !(r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || r == '-' || r == '_' || r == '.') {
			return false
		}
	}
	return true
}
func RouteAPI(protocolType protocol.ConversationPresetsProtocol) protocol.ConversationModelApi {
	switch protocolType {
	case "openai-responses":
		return protocol.ConversationModelApiResponses
	case "openai-chat-completions":
		return protocol.ConversationModelApiChatCompletions
	case "anthropic":
		return protocol.ConversationModelApiAnthropic
	case "gemini":
		return protocol.ConversationModelApiGemini
	case "deepseek":
		return protocol.ConversationModelApiDeepSeek
	case "opencode-go":
		return protocol.ConversationModelApiOpenCodeGo
	case "opencode-zen":
		return protocol.ConversationModelApiOpenCodeZen
	case "azure-openai":
		return protocol.ConversationModelApiAzureOpenAi
	case "azure-ai":
		return protocol.ConversationModelApiAzureAi
	}
	return protocol.ConversationModelApiPlugin
}
