// Request parameter policy is derived from Goose 14741e9a and rig-core 0.46.0.
// Apache-2.0; see LICENSE.goose and LICENSE.rig.
package model

import (
	"encoding/json"
	"sort"
	"strings"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const parametersKey = "sailry.parameters"

type callParameters struct {
	Body      map[string]any
	Raw       map[string]json.RawMessage
	Order     []string
	Remove    []string
	Developer bool
}

func (*callParameters) Options() {}
func requestParameters(binding Binding, call *fantasy.Call) *callParameters {
	params := &callParameters{Body: map[string]any{}, Raw: map[string]json.RawMessage{}, Remove: []string{"tool_choice"}}
	provider := binding.Provider
	model := metadata(binding)
	effort := binding.Config.Effort
	if effort.Kind == "default" && model != nil {
		effort = model.DefaultEffort
	}
	var reasoning *bool
	if model != nil {
		v := model.Reasoning
		reasoning = &v
		// A user's own tag outranks what discovery reported.
		if !model.CustomCapabilities && model.Capabilities != nil && model.Capabilities.Reasoning != nil {
			reasoning = model.Capabilities.Reasoning
		}
	}
	enabled := reasoning == nil || *reasoning
	if provider.Authentication == protocol.AuthenticationApiKey && (provider.Api == protocol.ConversationModelApiResponses || provider.Api == protocol.ConversationModelApiChatCompletions) {
		params.Body["prompt_cache_key"] = binding.Session.String()
	}
	var policies []protocol.ConversationPresetsRequestPolicy
	if provider.Binding != nil && *provider.Binding != nil {
		policies = (**provider.Binding).Policies
	} else if provider.Authentication == protocol.AuthenticationApiKey && provider.Api == protocol.ConversationModelApiChatCompletions {
		policies = bundledPolicies()
	}
	for _, policy := range policies {
		for _, id := range policy.Models {
			if id == binding.Config.Model {
				for key, raw := range policy.Body {
					var v any
					if json.Unmarshal(raw, &v) == nil {
						params.Body[key] = v
						params.Raw[key] = append(json.RawMessage(nil), raw...)
					}
				}
				break
			}
		}
	}
	level := effort.Kind
	if level == "default" || level == "budget" {
		level = ""
	}
	api := provider.Api
	if api == protocol.ConversationModelApiAzureAi {
		if strings.HasPrefix(strings.ToLower(binding.Config.Model), "claude") {
			api = protocol.ConversationModelApiAnthropic
		} else if responsesModel.MatchString(binding.Config.Model) {
			api = protocol.ConversationModelApiResponses
		} else {
			api = protocol.ConversationModelApiChatCompletions
		}
	}
	if api == protocol.ConversationModelApiOpenCodeGo || api == protocol.ConversationModelApiOpenCodeZen {
		if route, _ := transportBinding(binding); route.Kind == "responses" {
			api = protocol.ConversationModelApiResponses
		} else {
			api = protocol.ConversationModelApiChatCompletions
		}
	}
	switch api {
	case protocol.ConversationModelApiChatCompletions, protocol.ConversationModelApiResponses, protocol.ConversationModelApiDeepSeek, protocol.ConversationModelApiAzureOpenAi:
		openai := false
		leaf := binding.Config.Model[strings.LastIndex(binding.Config.Model, "/")+1:]
		if entry, ok := catalogEntry("openai", leaf); ok {
			openai = entry.Reasoning
		}
		if reasoning != nil {
			openai = openai && *reasoning
		}
		if openai || provider.Api == protocol.ConversationModelApiAzureAi && reasoning != nil && *reasoning {
			call.Temperature = nil
		}
		params.Developer = openai && api != protocol.ConversationModelApiResponses
		if enabled && level != "" && !(provider.Api == protocol.ConversationModelApiDeepSeek && level == "none") {
			params.Body["reasoning_effort"] = level
		}
		if level != "" && (provider.Api == protocol.ConversationModelApiDeepSeek || params.Body["thinking"] != nil) {
			thinking, _ := params.Body["thinking"].(map[string]any)
			if thinking == nil {
				thinking = map[string]any{}
			}
			kind := "enabled"
			if effort.Kind == "none" {
				kind = "disabled"
			}
			thinking["type"] = kind
			params.Body["thinking"] = thinking
		}
		if api == protocol.ConversationModelApiResponses {
			isReasoning := enabled && (reasoning != nil || openai || responsesModel.MatchString(binding.Config.Model))
			if isReasoning {
				params.Body["include"] = []string{"reasoning.encrypted_content"}
			}
			if value, ok := params.Body["reasoning_effort"]; ok {
				reasoning, _ := params.Body["reasoning"].(map[string]any)
				if reasoning == nil {
					reasoning = map[string]any{}
				}
				if _, exists := reasoning["effort"]; !exists {
					reasoning["effort"] = value
				}
				if _, exists := reasoning["summary"]; !exists {
					reasoning["summary"] = "auto"
				}
				params.Body["reasoning"] = reasoning
				delete(params.Body, "reasoning_effort")
			}
			if provider.Authentication != protocol.AuthenticationCopilot {
				if value := binding.Config.Features["speed"]; featureSupported(model, "speed", value) {
					switch value {
					case "fast":
						params.Body["service_tier"] = "priority"
					case "ultrafast":
						params.Body["service_tier"] = "ultrafast"
					}
				}
			}
		} else if openai && api != protocol.ConversationModelApiDeepSeek && provider.Api != protocol.ConversationModelApiAzureAi && call.MaxOutputTokens != nil {
			params.Body["max_completion_tokens"] = *call.MaxOutputTokens
			params.Remove = append(params.Remove, "max_tokens")
		}
		if effort.Kind == "budget" && (provider.Api == protocol.ConversationModelApiOpenCodeGo || provider.Api == protocol.ConversationModelApiOpenCodeZen) {
			params.Body["budget_tokens"] = effortBudget(effort)
		} else {
			params.Remove = append(params.Remove, "budget_tokens")
		}
	case protocol.ConversationModelApiAnthropic:
		entry, _ := catalogEntry("anthropic", binding.Config.Model)
		output := int64(4096)
		if call.MaxOutputTokens == nil {
			call.MaxOutputTokens = &output
		} else {
			output = *call.MaxOutputTokens
		}
		adaptive := entry.Rig.Adaptive
		always := adaptive && !entry.canDisable()
		on := enabled && (always || (effort.Kind != "default" && effort.Kind != "none") || effort.Kind == "default" && params.Body["budget_tokens"] != nil)
		if on && adaptive {
			if params.Body["thinking"] == nil {
				params.Body["thinking"] = map[string]any{"type": "adaptive"}
			}
			selected := level
			if selected == "" || selected == "none" {
				selected = "high"
			}
			if params.Body["output_config"] == nil {
				params.Body["output_config"] = map[string]any{"effort": selected}
			}
		} else if on {
			budget := int64(16000)
			switch effort.Kind {
			case "budget":
				budget = effortBudget(effort)
			case "minimal", "low":
				budget = 4000
			case "medium":
				budget = 10000
			case "xhigh", "max":
				budget = 32000
			}
			if value, ok := params.Body["budget_tokens"].(float64); ok {
				budget = int64(value)
			}
			if budget < 1024 {
				budget = 1024
			}
			if budget > output-1024 {
				budget = output - 1024
			}
			if budget >= 1024 && params.Body["thinking"] == nil {
				params.Body["thinking"] = map[string]any{"type": "enabled", "budget_tokens": budget}
			}
		} else if adaptive && !always && params.Body["thinking"] == nil {
			params.Body["thinking"] = map[string]any{"type": "disabled"}
		}
		if thinking, ok := params.Body["thinking"].(map[string]any); ok && strings.Contains(strings.ToLower(binding.Config.Model), "claude") && (thinking["type"] == "enabled" || thinking["type"] == "adaptive") {
			if _, ok := thinking["display"]; !ok {
				thinking["display"] = "summarized"
			}
		}
		params.Remove = append(params.Remove, "budget_tokens")
	case protocol.ConversationModelApiGemini:
		name := strings.ToLower(binding.Config.Model)
		var thinking map[string]any
		if !enabled || effort.Kind == "none" {
			if strings.HasPrefix(name, "gemini-3.5") || strings.HasPrefix(name, "gemini-3.6") {
				thinking = map[string]any{"thinkingLevel": "minimal", "includeThoughts": false}
			} else if strings.HasPrefix(name, "gemini-2.5-flash") {
				thinking = map[string]any{"thinkingBudget": 0, "includeThoughts": false}
			}
		} else if strings.HasPrefix(name, "gemini-3") && level != "" {
			selected := level
			switch level {
			case "minimal", "low":
				selected = "low"
			case "medium":
				if strings.HasPrefix(name, "gemini-3-pro") {
					selected = "low"
				}
			default:
				selected = "high"
			}
			thinking = map[string]any{"thinkingLevel": selected, "includeThoughts": true}
		} else if strings.HasPrefix(name, "gemini-2.5") {
			budget := int64(8192)
			if effort.Kind == "budget" && effortBudget(effort) >= 0 {
				budget = effortBudget(effort)
			}
			thinking = map[string]any{"thinkingBudget": budget, "includeThoughts": true}
		}
		if thinking != nil {
			generation, _ := params.Body["generationConfig"].(map[string]any)
			if generation == nil {
				generation = map[string]any{}
			}
			if generation["thinkingConfig"] == nil {
				generation["thinkingConfig"] = thinking
			}
			params.Body["generationConfig"] = generation
		}
		params.Remove = append(params.Remove, "thinking_budget")
	}
	appendOrder := func(key string) {
		for _, present := range params.Order {
			if present == key {
				return
			}
		}
		params.Order = append(params.Order, key)
	}
	// serde_json's preserved-order Map.remove uses swap removal. Retain source
	// insertion stages, including transient fields, so removing reasoning_effort
	// or a budget moves the same last member into its original position.
	removeOrder := func(key string) bool {
		for i, present := range params.Order {
			if present == key {
				params.Order[i] = params.Order[len(params.Order)-1]
				params.Order = params.Order[:len(params.Order)-1]
				return true
			}
		}
		return false
	}
	if effort.Kind == "budget" {
		if api == protocol.ConversationModelApiGemini {
			appendOrder("thinking_budget")
		} else {
			appendOrder("budget_tokens")
		}
	}
	if provider.Authentication == protocol.AuthenticationApiKey && (provider.Api == protocol.ConversationModelApiResponses || provider.Api == protocol.ConversationModelApiChatCompletions) {
		appendOrder("prompt_cache_key")
	}
	// Captured Go policy maps have canonical member order in their stored JSON.
	for _, policy := range policies {
		for _, id := range policy.Models {
			if id != binding.Config.Model {
				continue
			}
			var keys []string
			for key := range policy.Body {
				keys = append(keys, key)
			}
			sort.Strings(keys)
			for _, key := range keys {
				appendOrder(key)
			}
			break
		}
	}
	openaiAPI := api == protocol.ConversationModelApiResponses || api == protocol.ConversationModelApiChatCompletions || api == protocol.ConversationModelApiDeepSeek || api == protocol.ConversationModelApiAzureOpenAi
	if openaiAPI && enabled && level != "" {
		if provider.Api != protocol.ConversationModelApiDeepSeek || level != "none" {
			appendOrder("reasoning_effort")
		}
		if params.Body["thinking"] != nil {
			appendOrder("thinking")
		}
	}
	if api == protocol.ConversationModelApiAnthropic && effort.Kind == "xhigh" && params.Body["output_config"] != nil {
		appendOrder("output_config")
	}
	if api == protocol.ConversationModelApiResponses {
		if params.Body["service_tier"] != nil {
			appendOrder("service_tier")
		}
		appendOrder("store")
		appendOrder("prompt_cache_key")
		if params.Body["include"] != nil {
			appendOrder("include")
		}
		if removeOrder("reasoning_effort") {
			appendOrder("reasoning")
		}
	} else if openaiAPI && call.MaxOutputTokens != nil {
		if params.Body["max_completion_tokens"] != nil {
			appendOrder("max_completion_tokens")
		} else {
			appendOrder("max_tokens")
		}
	}
	if api == protocol.ConversationModelApiAnthropic {
		if params.Body["thinking"] != nil {
			appendOrder("thinking")
		}
		if params.Body["output_config"] != nil {
			appendOrder("output_config")
		}
	}
	if api == protocol.ConversationModelApiGemini {
		if params.Body["generationConfig"] != nil {
			appendOrder("generationConfig")
		}
		removeOrder("thinking_budget")
	} else if provider.Api != protocol.ConversationModelApiOpenCodeGo && provider.Api != protocol.ConversationModelApiOpenCodeZen {
		removeOrder("budget_tokens")
	}
	return params
}
func featureSupported(model *protocol.ConversationModel, id, value string) bool {
	if model == nil || model.Capabilities == nil {
		return false
	}
	for _, feature := range model.Capabilities.Features {
		if feature.ID == id {
			for _, option := range feature.Options {
				if option.ID == value {
					return true
				}
			}
		}
	}
	return false
}
