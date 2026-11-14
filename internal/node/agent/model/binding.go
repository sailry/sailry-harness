// Package model captures provider requests and owns per-attempt model transport.
package model

import (
	"context"
	"encoding/json"
	"net/url"
	"regexp"
	"slices"
	"strings"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/compact"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Binding struct {
	Provider protocol.ConversationProvider
	Config   protocol.SessionConfig
	Secret   *protocol.Secret
	Session  protocol.SessionID
}
type Tool = fantasy.FunctionTool
type Request struct {
	System          string
	Records         []canonical.Record
	Tools           []Tool
	ToolSchemas     map[string]json.RawMessage
	MaxOutputTokens *int64
	Temperature     *float64
	Headers         map[string]string
	Assets          map[string][]byte
	// Delegated marks a request of a delegated session, which an account
	// service counts as the agent's own.
	Delegated bool
}
type transportConfig struct {
	Kind, Model, Endpoint, Key string
	Headers                    map[string]string
	Query                      map[string]string
	RemoveHeaders              []string
	Order                      []string
	JSON                       map[string]any
	Remove                     []string
	OAuth                      bool
	Protection                 []protocol.Secret
}

// Capture retains independent copies of the immutable admitted configuration.
func Capture(provider protocol.ConversationProvider, config protocol.SessionConfig, secret *protocol.Secret, session protocol.SessionID) (Binding, error) {
	if !provider.Enabled {
		return Binding{}, &Failure{Kind: NotConfigured, Message: "model provider is disabled"}
	}
	if provider.ID != config.Provider || strings.TrimSpace(config.Model) == "" {
		return Binding{}, &Failure{Kind: NotConfigured, Message: "model selection is not configured"}
	}
	// A provider that lists models offers only those; an empty list offers any name.
	if len(provider.Models) > 0 && !slices.ContainsFunc(provider.Models, func(listed protocol.ConversationModel) bool { return listed.ID == config.Model }) {
		return Binding{}, &Failure{Kind: NotConfigured, Message: "model is not in the provider's model list"}
	}
	var result Binding
	// Protocol JSON is the capture boundary; nested maps and pointers cannot follow later saves.
	raw, err := json.Marshal(provider)
	if err != nil {
		return result, err
	}
	if err = json.Unmarshal(raw, &result.Provider); err != nil {
		return result, err
	}
	raw, err = json.Marshal(config)
	if err != nil {
		return result, err
	}
	if err = json.Unmarshal(raw, &result.Config); err != nil {
		return result, err
	}
	result.Session = session
	if secret != nil {
		value := protocol.NewSecret(secret.Expose())
		result.Secret = &value
	}
	if PluginBound(provider) {
		if (**provider.Binding).Package == nil {
			return Binding{}, &Failure{Kind: NotConfigured, Message: "captured model package is unavailable"}
		}
		if _, err := pluginAddress(provider.Endpoint); err != nil {
			return Binding{}, err
		}
	} else {
		if _, err := transportBinding(result); err != nil {
			return Binding{}, err
		}
	}
	return result, nil
}
func New(ctx context.Context, binding Binding) (fantasy.LanguageModel, error) {
	config, err := transportBinding(binding)
	if err != nil {
		return nil, err
	}
	if binding.Provider.Api == protocol.ConversationModelApiAzureAi {
		config, err = foundryBinding(ctx, binding, config)
		if err != nil {
			return nil, err
		}
	}
	discoverContext(ctx, binding, config)
	return newSDK(ctx, config)
}
func Scope(binding Binding) canonical.ModelSource {
	return canonical.ModelSource{ID: binding.Provider.ID, Name: binding.Provider.Name, API: binding.Provider.Api, Authentication: binding.Provider.Authentication, Model: binding.Config.Model}
}
func metadata(binding Binding) *protocol.ConversationModel {
	for i := range binding.Provider.Models {
		if binding.Provider.Models[i].ID == binding.Config.Model {
			return &binding.Provider.Models[i]
		}
	}
	return nil
}

// OutputLimit is the model's listed output limit, 0 when it lists none. A
// summary request asks for twice its handoff cap (compact.Output), which is
// 8,000 tokens for windows from 32,000 through 200,000, bounded by the model's
// limit. A model that lists none uses at most 8,000, still within the window's
// summary budget.
func OutputLimit(binding Binding, summary bool) int64 {
	var limit int64
	if model := metadata(binding); model != nil {
		limit = int64(model.Output)
	}
	if summary {
		if limit == 0 {
			limit = 8000
		}
		return min(limit, int64(compact.Output(uint64(ContextLimit(binding)))))
	}
	return limit
}
func Summary(binding Binding) Binding {
	result := binding
	result.Config.Effort = protocol.Effort{Kind: "default"}
	if model := metadata(binding); model != nil && model.Reasoning {
		rank := int64(1 << 62)
		for _, effort := range model.Efforts {
			if value, ok := effortRank(effort); ok && value < rank {
				rank = value
				result.Config.Effort = effort
			}
		}
	}
	if result.Config.Effort.Kind == "budget" {
		budget := effortBudget(result.Config.Effort)
		if max := OutputLimit(binding, true) / 2; budget > max {
			budget = max
		}
		value := int32(budget)
		result.Config.Effort.Data = &value
	}
	return result
}
func effortBudget(e protocol.Effort) int64 {
	switch v := e.Data.(type) {
	case int32:
		return int64(v)
	case *int32:
		if v != nil {
			return int64(*v)
		}
	}
	return 0
}
func effortRank(e protocol.Effort) (int64, bool) {
	switch e.Kind {
	case "none":
		return 0, true
	case "minimal":
		return 1, true
	case "low":
		return 2, true
	case "medium":
		return 3, true
	case "high":
		return 4, true
	case "xhigh":
		return 5, true
	case "max":
		return 6, true
	case "budget":
		v := effortBudget(e)
		return 100 + v, v > 0
	}
	return 0, false
}
func endpoint(value string) (*url.URL, error) {
	value = strings.TrimSpace(value)
	if value == "" || len(value) > 2048 || strings.HasPrefix(value, "/") {
		return nil, &Failure{Kind: InvalidRequest, Message: "provider endpoint is invalid"}
	}
	if !strings.Contains(value, "://") {
		value = "https://" + value
	}
	u, err := url.Parse(value)
	if err != nil || u.Hostname() == "" || (u.Scheme != "http" && u.Scheme != "https") || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
		return nil, &Failure{Kind: InvalidRequest, Message: "provider endpoint must be an HTTP URL without credentials, query or fragment"}
	}
	return u, nil
}

var responsesModel = regexp.MustCompile(`(?i)(?:^|[-/])(?:o\d+(?:$|-)|gpt-(?:5|6)(?:$|[-.]))`)

func gatewayPath(path string) string {
	path = strings.TrimRight(path, "/")
	for _, suffix := range []string{"/chat/completions", "/responses"} {
		if strings.HasSuffix(path, suffix) {
			return strings.TrimSuffix(path, suffix)
		}
	}
	last := path[strings.LastIndex(path, "/")+1:]
	if matched, _ := regexp.MatchString(`^v[0-9]+$`, last); matched {
		return path
	}
	return path + "/v1"
}
func transportBinding(binding Binding) (transportConfig, error) {
	provider := binding.Provider
	u, err := endpoint(provider.Endpoint)
	if err != nil {
		return transportConfig{}, err
	}
	config := transportConfig{Model: binding.Config.Model, Endpoint: u.String(), Headers: map[string]string{"User-Agent": "Sailry/0.1.0"}, Query: map[string]string{}, JSON: map[string]any{}, Remove: []string{"tool_choice"}, RemoveHeaders: []string{"Authorization", "X-Api-Key", "X-Goog-Api-Key", "Api-Key"}}
	if binding.Secret != nil {
		config.Key = binding.Secret.Expose()
	}
	var account *providers.AccountGrant
	if provider.Authentication != protocol.AuthenticationApiKey {
		grant, headers, err := providers.AccountHeaders(provider, binding.Secret)
		if err != nil {
			return config, err
		}
		account = &grant
		config.Protection = grant.Secrets()
		config.Key = ""
		for name, values := range headers {
			config.Headers[name] = strings.Join(values, ", ")
		}
		config.OAuth = provider.Authentication == protocol.AuthenticationClaudeOAuth
		if config.OAuth {
			installation, accountID := "", ""
			if grant.InstallationID != nil {
				installation = *grant.InstallationID
			}
			if grant.AccountID != nil {
				accountID = *grant.AccountID
			}
			config.JSON["metadata"] = claudeMetadata(installation, binding.Session.String(), accountID)
		}
	}
	api := provider.Api
	if api == protocol.ConversationModelApiAzureOpenAi {
		if provider.Options == nil || provider.Options.Kind != "azure_open_ai" {
			return config, &Failure{Kind: NotConfigured, Message: "Azure API version is required"}
		}
		var version string
		switch v := provider.Options.Data.(type) {
		case protocol.ConversationConnectionOptionsAzureOpenAiPayload:
			version = v.ApiVersion
		case *protocol.ConversationConnectionOptionsAzureOpenAiPayload:
			if v != nil {
				version = v.ApiVersion
			}
		}
		if !safeSegment(version) || !safeSegment(config.Model) {
			return config, &Failure{Kind: InvalidRequest, Message: "Azure connection options are invalid"}
		}
		path := strings.Trim(u.Path, "/")
		if strings.Contains(path, "/deployments/") || strings.HasPrefix(path, "deployments/") {
			path = strings.TrimSuffix(path, "/chat/completions")
		} else {
			path = strings.TrimSuffix(path, "/openai")
			if path == "openai" {
				path = ""
			}
			path += "/openai/deployments/" + config.Model
		}
		u.Path = "/" + strings.TrimLeft(path, "/")
		config.Query["api-version"] = version
		api = protocol.ConversationModelApiChatCompletions
	}
	if api == protocol.ConversationModelApiOpenCodeGo || api == protocol.ConversationModelApiOpenCodeZen {
		path := gatewayPath(u.Path)
		response := (path == "/v1" && responsesModel.MatchString(config.Model)) || strings.HasSuffix(strings.TrimRight(u.Path, "/"), "/responses")
		u.Path = path
		if api == protocol.ConversationModelApiOpenCodeGo {
			config.Headers["x-opencode-session"] = binding.Session.String()
		}
		api = protocol.ConversationModelApiChatCompletions
		if response {
			api = protocol.ConversationModelApiResponses
		}
	}
	switch api {
	case protocol.ConversationModelApiAnthropic:
		config.Kind = "anthropic"
	case protocol.ConversationModelApiResponses:
		config.Kind = "responses"
		u.Path = strings.TrimSuffix(strings.TrimRight(u.Path, "/"), "/responses")
	case protocol.ConversationModelApiGemini:
		config.Kind = "google"
		u.Path = strings.TrimSuffix(strings.TrimRight(u.Path, "/"), "/v1beta")
	case protocol.ConversationModelApiChatCompletions, protocol.ConversationModelApiDeepSeek, protocol.ConversationModelApiAzureAi:
		config.Kind = "chat"
		u.Path = strings.TrimSuffix(strings.TrimRight(u.Path, "/"), "/chat/completions")
	default:
		return config, &Failure{Kind: Unsupported, Message: "model transport is not configured"}
	}
	config.Endpoint = strings.TrimRight(u.String(), "/")
	key := config.Key
	if account != nil {
		if provider.Api == protocol.ConversationModelApiAnthropic {
			config.Headers["anthropic-version"] = "2023-06-01"
		}
		if provider.Authentication == protocol.AuthenticationChatGptOAuth {
			config.JSON["store"] = false
			config.Remove = append(config.Remove, "max_output_tokens", "temperature")
		}
		remove := config.RemoveHeaders[:0]
		for _, name := range config.RemoveHeaders {
			keep := true
			for configured := range config.Headers {
				if strings.EqualFold(name, configured) {
					keep = false
				}
			}
			if keep {
				remove = append(remove, name)
			}
		}
		config.RemoveHeaders = remove
		return config, nil
	}
	header, value := "authorization", "Bearer "+key
	switch provider.Api {
	case protocol.ConversationModelApiAnthropic:
		header, value = "x-api-key", key
	case protocol.ConversationModelApiGemini:
		header, value = "x-goog-api-key", key
	case protocol.ConversationModelApiAzureAi, protocol.ConversationModelApiAzureOpenAi:
		header, value = "api-key", key
	}
	if provider.Binding != nil && *provider.Binding != nil {
		if injection := (**provider.Binding).KeyInjection; injection != nil {
			header, value = injection.Header, strings.ReplaceAll(injection.Template, "{key}", key)
		}
	}
	if key != "" {
		config.Headers[header] = value
		var remove []string
		for _, name := range config.RemoveHeaders {
			if !strings.EqualFold(name, header) {
				remove = append(remove, name)
			}
		}
		config.RemoveHeaders = remove
	}
	if provider.Api == protocol.ConversationModelApiAnthropic || provider.Api == protocol.ConversationModelApiAzureAi {
		config.Headers["anthropic-version"] = "2023-06-01"
	}
	if config.Kind == "google" && (!strings.EqualFold(header, "x-goog-api-key") || value != key) {
		return config, &Failure{Kind: NotConfigured, Message: "Gemini requires API key authentication"}
	}
	return config, nil
}
func safeSegment(value string) bool {
	if value == "" || len(value) > 128 || value == "." || value == ".." {
		return false
	}
	for _, c := range value {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '-' || c == '_' || c == '.') {
			return false
		}
	}
	return true
}
