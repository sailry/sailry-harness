package providers

import (
	"context"
	"encoding/json"
	"net/http"
	"net/url"
	"reflect"
	"sort"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func Discover(ctx context.Context, provider protocol.ConversationProvider, secret *protocol.Secret) (protocol.ConversationDiscoveryCatalog, error) {
	var catalog protocol.ConversationDiscoveryCatalog
	if provider.Api == protocol.ConversationModelApiPlugin {
		return catalog, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "custom model discovery requires its captured translation"}
	}
	if provider.Authentication != protocol.AuthenticationApiKey {
		return discoverAccount(ctx, provider, secret)
	}
	if provider.Api == protocol.ConversationModelApiAzureAi || provider.Api == protocol.ConversationModelApiAzureOpenAi {
		return catalog, invalid("cloud deployments require an explicit model ID")
	}
	root, err := Endpoint(provider.Endpoint)
	if err != nil {
		return catalog, err
	}
	candidates := []url.URL{*root}
	path := strings.TrimRight(root.Path, "/")
	if provider.Api == protocol.ConversationModelApiAnthropic {
		candidates[0].Path = strings.TrimSuffix(path, "/v1")
	} else {
		last := path[strings.LastIndex(path, "/")+1:]
		versioned := len(last) > 1 && last[0] == 'v' && last[1] >= '0' && last[1] <= '9'
		if !versioned {
			var versions []string
			switch provider.Api {
			case protocol.ConversationModelApiChatCompletions, protocol.ConversationModelApiResponses, protocol.ConversationModelApiDeepSeek:
				versions = []string{"v1"}
			case protocol.ConversationModelApiGemini:
				versions = []string{"v1beta", "v1"}
			}
			for _, version := range versions {
				candidate := *root
				candidate.Path = path + "/" + version
				candidates = append(candidates, candidate)
			}
		}
	}
	auth, err := headers(provider, secret)
	if err != nil {
		return catalog, err
	}
	owner, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	client := httpClient(15 * time.Second)
	remaining := int64(8 * 1024 * 1024)
	for _, candidate := range candidates {
		endpoint := strings.TrimRight(candidate.String(), "/")
		suffix := "/models"
		if provider.Api == protocol.ConversationModelApiAnthropic {
			suffix = "/v1/models"
		}
		candidate.Path = strings.TrimRight(candidate.Path, "/") + suffix
		models, found, err := discoverPages(owner, client, provider.Api, candidate, auth, &remaining)
		if err != nil {
			if ctx.Err() != nil {
				return catalog, cancelled("model discovery cancelled")
			}
			return catalog, err
		}
		if found {
			return protocol.ConversationDiscoveryCatalog{Endpoint: endpoint, Models: models}, nil
		}
	}
	return catalog, unavailable("provider model endpoint was not found")
}
func discoverPages(ctx context.Context, client *http.Client, api protocol.ConversationModelApi, endpoint url.URL, headers http.Header, remaining *int64) ([]protocol.ConversationDiscoveryModel, bool, error) {
	models := map[string]protocol.ConversationDiscoveryModel{}
	cursors := map[string]bool{}
	cursor := ""
	for page := 0; page < 20; page++ {
		target := endpoint
		query := target.Query()
		switch api {
		case protocol.ConversationModelApiAnthropic:
			query.Set("limit", "100")
			if cursor != "" {
				query.Set("after_id", cursor)
			}
		case protocol.ConversationModelApiGemini:
			query.Set("pageSize", "100")
			if cursor != "" {
				query.Set("pageToken", cursor)
			}
		}
		target.RawQuery = query.Encode()
		request, err := http.NewRequestWithContext(ctx, http.MethodGet, target.String(), nil)
		if err != nil {
			return nil, false, err
		}
		request.Header = headers.Clone()
		response, err := client.Do(request)
		if err != nil {
			return nil, false, unavailable("provider request failed")
		}
		media := strings.TrimSpace(strings.Split(response.Header.Get("Content-Type"), ";")[0])
		if cursor == "" && (response.StatusCode == 404 || response.StatusCode == 405 || response.StatusCode >= 200 && response.StatusCode < 300 && strings.EqualFold(media, "text/html")) {
			response.Body.Close()
			return nil, false, nil
		}
		limit := int64(2 * 1024 * 1024)
		if *remaining < limit {
			limit = *remaining
		}
		raw, err := readJSON(response, limit)
		if err != nil {
			return nil, false, err
		}
		*remaining -= int64(len(raw))
		value := object(raw)
		if api == protocol.ConversationModelApiGemini && cursor == "" && value["models"] == nil && string(value["object"]) == `"list"` && value["data"] != nil {
			return nil, false, nil
		}
		entries, next, err := parseDiscovery(api, value)
		if err != nil {
			return nil, false, err
		}
		for _, model := range entries {
			if old, found := models[model.ID]; found && !reflect.DeepEqual(old, model) {
				return nil, false, unavailable("provider returned conflicting model metadata")
			}
			models[model.ID] = model
			if len(models) > 1000 {
				return nil, false, unavailable("provider model list exceeds its limit")
			}
		}
		if next == nil {
			result := make([]protocol.ConversationDiscoveryModel, 0, len(models))
			for _, model := range models {
				result = append(result, model)
			}
			sort.Slice(result, func(i, j int) bool { return result[i].ID < result[j].ID })
			return result, true, nil
		}
		if *next == "" || len(*next) > 2048 || cursors[*next] {
			return nil, false, unavailable("provider returned an invalid pagination cursor")
		}
		cursor = *next
		cursors[cursor] = true
	}
	return nil, false, unavailable("provider model pagination exceeds its limit")
}
func parseDiscovery(api protocol.ConversationModelApi, value map[string]json.RawMessage) ([]protocol.ConversationDiscoveryModel, *string, error) {
	key := "data"
	if api == protocol.ConversationModelApiGemini {
		key = "models"
	}
	var entries []map[string]json.RawMessage
	if json.Unmarshal(value[key], &entries) != nil || entries == nil {
		return nil, nil, unavailable("provider model list has invalid shape")
	}
	result := []protocol.ConversationDiscoveryModel{}
	for _, entry := range entries {
		gemini := api == protocol.ConversationModelApiGemini
		if gemini && entry["supportedGenerationMethods"] != nil && string(entry["supportedGenerationMethods"]) != "null" {
			var methods []string
			if json.Unmarshal(entry["supportedGenerationMethods"], &methods) != nil {
				return nil, nil, unavailable("provider generation methods are invalid")
			}
			generate := false
			for _, method := range methods {
				generate = generate || method == "generateContent"
			}
			if !generate {
				continue
			}
		}
		idKey := "id"
		if gemini {
			idKey = "name"
		}
		model := protocol.DefaultConversationDiscoveryModel()
		if json.Unmarshal(entry[idKey], &model.ID) != nil {
			return nil, nil, unavailable("provider model identifier is missing")
		}
		if gemini {
			model.ID = strings.TrimPrefix(model.ID, "models/")
		}
		if !identifier(model.ID, 256) {
			return nil, nil, unavailable("provider model identifier is invalid")
		}
		contextKeys, outputKeys := []string{"context_window_tokens", "context_window", "context_length"}, []string{"max_output_tokens", "max_completion_tokens"}
		if gemini {
			contextKeys, outputKeys = []string{"inputTokenLimit"}, []string{"outputTokenLimit"}
		} else if api == protocol.ConversationModelApiAnthropic {
			contextKeys = []string{"max_input_tokens", "context_window_tokens", "context_window", "context_length"}
			outputKeys = []string{"max_tokens", "max_output_tokens", "max_completion_tokens"}
		}
		var err error
		model.Context, err = discoveryLimit(entry, contextKeys)
		if err != nil {
			return nil, nil, err
		}
		model.Output, err = discoveryLimit(entry, outputKeys)
		if err != nil {
			return nil, nil, err
		}
		model.Capabilities, err = discoveryCapabilities(entry)
		if err != nil {
			return nil, nil, err
		}
		result = append(result, model)
	}
	var next *string
	switch api {
	case protocol.ConversationModelApiGemini:
		var err error
		next, err = optionalString(value["nextPageToken"])
		if err != nil {
			return nil, nil, err
		}
		if next != nil && *next == "" {
			next = nil
		}
	case protocol.ConversationModelApiAnthropic:
		var more bool
		if field := value["has_more"]; field == nil {
			if string(value["object"]) != `"list"` {
				return nil, nil, unavailable("provider pagination state is invalid")
			}
		} else if json.Unmarshal(field, &more) != nil {
			return nil, nil, unavailable("provider pagination state is invalid")
		}
		if more {
			var err error
			next, err = optionalString(value["last_id"])
			if err != nil || next == nil {
				return nil, nil, unavailable("provider pagination cursor is missing")
			}
		}
	}
	return result, next, nil
}
func discoveryLimit(entry map[string]json.RawMessage, keys []string) (*uint32, error) {
	var found *uint32
	for _, key := range keys {
		if raw := entry[key]; len(raw) > 0 && string(raw) != "null" {
			var value uint32
			if json.Unmarshal(raw, &value) != nil || value == 0 {
				return nil, unavailable("provider returned invalid model limits")
			}
			if found != nil && *found != value {
				return nil, unavailable("provider returned conflicting model limits")
			}
			found = &value
		}
	}
	return found, nil
}
func optionalString(raw json.RawMessage) (*string, error) {
	if len(raw) == 0 || string(raw) == "null" {
		return nil, nil
	}
	var value string
	if json.Unmarshal(raw, &value) != nil {
		return nil, unavailable("provider pagination cursor is invalid")
	}
	return &value, nil
}
func discoveryCapabilities(entry map[string]json.RawMessage) (*protocol.ConversationDiscoveryCapabilities, error) {
	value := protocol.DefaultConversationDiscoveryCapabilities()
	nested := object(entry["capabilities"])
	for _, item := range []struct {
		key    string
		target **bool
	}{{"vision", &value.Vision}, {"tool_call", &value.Tools}, {"reasoning", &value.Reasoning}} {
		raw := entry[item.key]
		if len(raw) == 0 || string(raw) == "null" {
			raw = nested[item.key]
		}
		if len(raw) > 0 && string(raw) != "null" {
			var flag bool
			if json.Unmarshal(raw, &flag) != nil {
				return nil, unavailable("provider model capability is invalid")
			}
			*item.target = &flag
		}
	}
	if raw := entry["supported_reasoning_levels"]; len(raw) > 0 && string(raw) != "null" {
		var choices []struct {
			Effort string `json:"effort"`
		}
		if json.Unmarshal(raw, &choices) != nil || len(choices) > 32 {
			return nil, unavailable("provider reasoning choices are invalid")
		}
		efforts := []protocol.Effort{}
		seen := map[string]bool{}
		for _, choice := range choices {
			if !identifier(choice.Effort, 128) {
				return nil, unavailable("provider reasoning choice is invalid")
			}
			if validEffort(choice.Effort) {
				if seen[choice.Effort] {
					return nil, unavailable("provider reasoning choices are duplicated")
				}
				seen[choice.Effort] = true
				efforts = append(efforts, protocol.Effort{Kind: choice.Effort})
			}
		}
		value.Efforts = &efforts
		if value.Reasoning == nil {
			reasoning := len(choices) > 0
			value.Reasoning = &reasoning
		}
	}
	if raw := entry["default_reasoning_level"]; len(raw) > 0 && string(raw) != "null" {
		var name string
		if json.Unmarshal(raw, &name) != nil || !identifier(name, 128) {
			return nil, unavailable("provider reasoning choice is invalid")
		}
		if validEffort(name) {
			effort := protocol.Effort{Kind: name}
			value.DefaultEffort = &effort
			if value.Efforts != nil {
				found := false
				for _, choice := range *value.Efforts {
					found = found || choice.Kind == name
				}
				if !found {
					return nil, unavailable("provider default reasoning choice is unsupported")
				}
			}
		}
	}
	if value.Vision == nil && value.Tools == nil && value.Reasoning == nil && value.Efforts == nil && value.DefaultEffort == nil {
		return nil, nil
	}
	return &value, nil
}
func validEffort(name string) bool {
	switch name {
	case "none", "minimal", "low", "medium", "high", "xhigh", "max":
		return true
	}
	return false
}
