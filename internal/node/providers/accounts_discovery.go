package providers

import (
	"context"
	"encoding/json"
	"net/http"
	"reflect"
	"sort"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func accountDiscoveryRequest(ctx context.Context, provider protocol.ConversationProvider, secret *protocol.Secret) (*http.Request, error) {
	endpoint, err := Endpoint(provider.Endpoint)
	if err != nil {
		return nil, err
	}
	_, headers, err := AccountHeaders(provider, secret)
	if err != nil {
		return nil, err
	}
	endpoint.Path = strings.TrimRight(endpoint.Path, "/") + "/models"
	switch provider.Authentication {
	case protocol.AuthenticationChatGptOAuth:
		fields, err := OAuthFields(provider)
		if err != nil {
			return nil, err
		}
		query := endpoint.Query()
		query.Set("client_version", fields["catalog_version"])
		endpoint.RawQuery = query.Encode()
	case protocol.AuthenticationCopilot:
		headers.Set("openai-intent", "conversation-panel")
		headers.Set("x-initiator", "user")
		headers.Set("x-request-id", protocol.NewRequestID().String())
	case protocol.AuthenticationChatGpt, protocol.AuthenticationPlugin:
	default:
		return nil, invalid("account authorization uses the native model catalog")
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint.String(), nil)
	if err != nil {
		return nil, err
	}
	request.Header = headers
	return request, nil
}

func discoverAccount(ctx context.Context, provider protocol.ConversationProvider, secret *protocol.Secret) (protocol.ConversationDiscoveryCatalog, error) {
	catalog := protocol.ConversationDiscoveryCatalog{Endpoint: strings.TrimRight(provider.Endpoint, "/"), Models: []protocol.ConversationDiscoveryModel{}}
	owner, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	client, remaining := httpClient(15*time.Second), int64(8*1024*1024)
	if provider.Authentication == protocol.AuthenticationClaudeOAuth || provider.Authentication == protocol.AuthenticationPlugin && provider.Api == protocol.ConversationModelApiAnthropic {
		endpoint, err := Endpoint(provider.Endpoint)
		if err != nil {
			return catalog, err
		}
		endpoint.Path = strings.TrimSuffix(strings.TrimRight(endpoint.Path, "/"), "/v1") + "/v1/models"
		auth, err := headers(provider, secret)
		if err != nil {
			return catalog, err
		}
		models, found, err := discoverPages(owner, client, provider.Api, *endpoint, auth, &remaining)
		if err != nil {
			if ctx.Err() != nil {
				return catalog, cancelled("model discovery cancelled")
			}
			return catalog, err
		}
		if !found {
			return catalog, unavailable("provider model endpoint was not found")
		}
		catalog.Models = models
		return catalog, nil
	}
	request, err := accountDiscoveryRequest(owner, provider, secret)
	if err != nil {
		return catalog, err
	}
	response, err := client.Do(request)
	if err != nil {
		if ctx.Err() != nil {
			return catalog, cancelled("model discovery cancelled")
		}
		return catalog, unavailable("provider request failed")
	}
	body, err := readJSON(response, 2*1024*1024)
	if err != nil {
		return catalog, err
	}
	catalog.Models, err = parseAccountDiscovery(provider.Authentication, provider.Api, object(body))
	return catalog, err
}

func parseAccountDiscovery(authentication protocol.Authentication, api protocol.ConversationModelApi, document map[string]json.RawMessage) ([]protocol.ConversationDiscoveryModel, error) {
	chatgpt := authentication == protocol.AuthenticationChatGpt || authentication == protocol.AuthenticationChatGptOAuth
	key := "data"
	if chatgpt {
		key = "models"
	}
	var entries []map[string]json.RawMessage
	if json.Unmarshal(document[key], &entries) != nil || entries == nil {
		return nil, unavailable("provider model list has invalid shape")
	}
	if len(entries) > 1000 {
		return nil, unavailable("provider model list exceeds its limit")
	}
	models, indices := []protocol.ConversationDiscoveryModel{}, map[string]int{}
	for _, entry := range entries {
		var visibility string
		_ = json.Unmarshal(entry["visibility"], &visibility)
		if chatgpt && visibility != "list" {
			continue
		}
		if !chatgpt {
			supported, err := accountSupports(entry, api)
			if err != nil {
				return nil, err
			}
			if !supported {
				continue
			}
		}
		model := protocol.DefaultConversationDiscoveryModel()
		idKey := "id"
		if chatgpt {
			idKey = "slug"
		}
		if json.Unmarshal(entry[idKey], &model.ID) != nil || !identifier(model.ID, 256) {
			return nil, unavailable("provider model identifier is invalid")
		}
		capabilities := protocol.DefaultConversationDiscoveryCapabilities()
		var err error
		if chatgpt {
			var display string
			if json.Unmarshal(entry["display_name"], &display) != nil || !identifier(display, 256) {
				return nil, unavailable("provider model display name is invalid")
			}
			model.DisplayName = &display
			model.Context, err = discoveryLimit(entry, []string{"context_window"})
			if err == nil {
				model.Output, err = discoveryLimit(entry, []string{"max_output_tokens"})
			}
			if err != nil {
				return nil, err
			}
			capabilities.Reasoning, capabilities.Efforts, err = accountEfforts(entry["supported_reasoning_levels"], true)
			if err != nil {
				return nil, err
			}
			if raw := entry["default_reasoning_level"]; len(raw) != 0 && string(raw) != "null" {
				capabilities.DefaultEffort, err = accountEffort(raw)
				if err != nil {
					return nil, err
				}
			}
			if capabilities.DefaultEffort != nil && capabilities.Efforts != nil {
				found := false
				for _, choice := range *capabilities.Efforts {
					found = found || reflect.DeepEqual(choice, *capabilities.DefaultEffort)
				}
				if !found {
					return nil, unavailable("provider reasoning default is not supported")
				}
			}
			if raw := entry["input_modalities"]; len(raw) != 0 && string(raw) != "null" {
				modalities, err := accountStrings(raw)
				if err != nil {
					return nil, err
				}
				vision := false
				for _, modality := range modalities {
					vision = vision || modality == "image"
				}
				capabilities.Vision = &vision
			}
			if authentication == protocol.AuthenticationChatGptOAuth {
				capabilities.Features, err = accountFeatures(entry["service_tiers"])
				if err != nil {
					return nil, err
				}
			}
		} else {
			metadata := object(entry["capabilities"])
			limits, supports := object(metadata["limits"]), object(metadata["supports"])
			model.Context, err = discoveryLimit(limits, []string{"max_context_window_tokens"})
			if err == nil {
				model.Output, err = discoveryLimit(limits, []string{"max_output_tokens"})
			}
			if err != nil {
				return nil, err
			}
			capabilities.Reasoning, capabilities.Efforts, err = accountEfforts(supports["reasoning_effort"], false)
			if err != nil {
				return nil, err
			}
			for _, field := range []struct {
				name   string
				target **bool
			}{{"vision", &capabilities.Vision}, {"tool_calls", &capabilities.Tools}} {
				if raw := supports[field.name]; len(raw) != 0 && string(raw) != "null" {
					var flag bool
					if json.Unmarshal(raw, &flag) != nil {
						return nil, unavailable("provider model capability is invalid")
					}
					*field.target = &flag
				}
			}
		}
		if model.Context != nil && model.Output != nil && *model.Output > *model.Context {
			return nil, unavailable("provider model limits are inconsistent")
		}
		model.Capabilities = &capabilities
		if index, found := indices[model.ID]; found {
			if !reflect.DeepEqual(models[index], model) {
				return nil, unavailable("provider returned conflicting model metadata")
			}
		} else {
			indices[model.ID] = len(models)
			models = append(models, model)
		}
	}
	if !chatgpt {
		sort.Slice(models, func(left, right int) bool { return models[left].ID < models[right].ID })
	}
	return models, nil
}

func accountSupports(entry map[string]json.RawMessage, api protocol.ConversationModelApi) (bool, error) {
	metadata := object(entry["capabilities"])
	if string(metadata["type"]) != `"chat"` {
		return false, nil
	}
	raw := entry["supported_endpoints"]
	if len(raw) == 0 || string(raw) == "null" {
		return api == protocol.ConversationModelApiChatCompletions, nil
	}
	wanted := ""
	switch api {
	case protocol.ConversationModelApiChatCompletions:
		wanted = "/chat/completions"
	case protocol.ConversationModelApiResponses:
		wanted = "/responses"
	default:
		return false, nil
	}
	endpoints, err := accountStrings(raw)
	if err != nil {
		return false, err
	}
	for _, endpoint := range endpoints {
		if endpoint == wanted {
			return true, nil
		}
	}
	return false, nil
}

func accountStrings(raw json.RawMessage) ([]string, error) {
	var values []string
	if json.Unmarshal(raw, &values) != nil || values == nil || len(values) > 32 {
		return nil, unavailable("provider model choices are invalid")
	}
	for _, value := range values {
		if !identifier(value, 128) {
			return nil, unavailable("provider model choice is invalid")
		}
	}
	return values, nil
}

func accountEffort(raw json.RawMessage) (*protocol.Effort, error) {
	var value string
	if json.Unmarshal(raw, &value) != nil || !identifier(value, 128) {
		return nil, unavailable("provider reasoning choice is invalid")
	}
	if !validEffort(value) {
		return nil, nil
	}
	return &protocol.Effort{Kind: value}, nil
}

func accountEfforts(raw json.RawMessage, objects bool) (*bool, *[]protocol.Effort, error) {
	if len(raw) == 0 || string(raw) == "null" {
		return nil, nil, nil
	}
	var values []json.RawMessage
	if json.Unmarshal(raw, &values) != nil || values == nil || len(values) > 32 {
		return nil, nil, unavailable("provider reasoning choices are invalid")
	}
	choices, seen := []protocol.Effort{}, map[string]bool{}
	for _, value := range values {
		if objects {
			value = object(value)["effort"]
		}
		choice, err := accountEffort(value)
		if err != nil {
			return nil, nil, err
		}
		if choice != nil {
			if seen[choice.Kind] {
				return nil, nil, unavailable("provider reasoning choices are duplicated")
			}
			seen[choice.Kind] = true
			choices = append(choices, *choice)
		}
	}
	reasoning := len(values) != 0
	return &reasoning, &choices, nil
}

const accountFastIcon = "reicon:newicons/flash2"
const accountUltrafastIcon = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M8.5 2 2.5 13.5h5L6.5 22l6-11.5h-5z"/><path d="M17.5 2l-6 11.5h5L15.5 22l6-11.5h-5z"/></svg>`

func accountFeatures(raw json.RawMessage) ([]protocol.ConversationFeaturesFeature, error) {
	result := []protocol.ConversationFeaturesFeature{}
	if len(raw) == 0 || string(raw) == "null" {
		return result, nil
	}
	var tiers []struct {
		ID string `json:"id"`
	}
	if json.Unmarshal(raw, &tiers) != nil || tiers == nil || len(tiers) > 16 {
		return nil, unavailable("provider service tiers are invalid")
	}
	seen := map[string]bool{}
	for _, tier := range tiers {
		if !identifier(tier.ID, 64) || seen[tier.ID] {
			return nil, unavailable("provider service tier identifiers conflict")
		}
		seen[tier.ID] = true
	}
	name, fast, ultrafast := "Speed", "Fast", "Ultrafast"
	feature := protocol.ConversationFeaturesFeature{ID: "speed", Name: &name, Icon: &protocol.PluginDesktopIcon{Kind: "Name", Data: accountFastIcon}}
	if seen["priority"] {
		feature.Options = append(feature.Options, protocol.ConversationFeaturesFeatureOption{ID: "fast", Name: &fast, Icon: &protocol.PluginDesktopIcon{Kind: "Name", Data: accountFastIcon}})
	}
	if seen["ultrafast"] {
		feature.Options = append(feature.Options, protocol.ConversationFeaturesFeatureOption{ID: "ultrafast", Name: &ultrafast, Icon: &protocol.PluginDesktopIcon{Kind: "Svg", Data: protocol.PluginDesktopIconSvgPayload{Svg: accountUltrafastIcon}}})
	}
	if len(feature.Options) != 0 {
		result = append(result, feature)
	}
	return result, nil
}
