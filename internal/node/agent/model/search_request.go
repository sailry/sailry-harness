package model

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/url"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// PrepareNativeSearch shares the captured model's authorization and declared
// request policies. Native search owns its input and tool fields.
func PrepareNativeSearch(ctx context.Context, binding Binding, body map[string]any) (*http.Request, error) {
	config, err := transportBinding(binding)
	if err != nil {
		return nil, err
	}
	endpoint, err := url.Parse(config.Endpoint)
	if err != nil {
		return nil, err
	}
	path := strings.TrimRight(endpoint.Path, "/")
	switch binding.Provider.Api {
	case protocol.ConversationModelApiResponses:
		if !strings.HasSuffix(path, "/responses") {
			path += "/responses"
		}
	case protocol.ConversationModelApiAnthropic:
		path = strings.TrimSuffix(strings.TrimSuffix(path, "/v1/messages"), "/v1") + "/v1/messages"
	case protocol.ConversationModelApiGemini:
		name := strings.TrimPrefix(binding.Config.Model, "models/")
		if name == "" || strings.ContainsAny(name, "/:") || strings.ContainsFunc(name, unicode.IsControl) {
			return nil, &Failure{Kind: InvalidRequest, Message: "Gemini search model identifier is invalid"}
		}
		if !strings.HasSuffix(path, "/v1beta") && !strings.HasSuffix(path, "/v1") {
			path += "/v1beta"
		}
		path += "/models/" + url.PathEscape(name) + ":generateContent"
	default:
		return nil, &Failure{Kind: Unsupported, Message: "provider does not support native web search"}
	}
	endpoint.Path = path
	if binding.Provider.Binding != nil && *binding.Provider.Binding != nil {
		for _, policy := range (**binding.Provider.Binding).Policies {
			for _, name := range policy.Models {
				if name != binding.Config.Model {
					continue
				}
				for key, value := range policy.Body {
					switch key {
					case "tools", "tool_choice", "input", "messages", "contents", "stream", "store", "include", "system", "systemInstruction":
						continue
					}
					var decoded any
					if err := json.Unmarshal(value, &decoded); err != nil {
						return nil, err
					}
					body[key] = decoded
				}
			}
		}
	}
	encoded, err := json.Marshal(body)
	if err != nil {
		return nil, err
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint.String(), bytes.NewReader(encoded))
	if err != nil {
		return nil, err
	}
	request.Header.Set("Content-Type", "application/json")
	for key, value := range config.Headers {
		request.Header.Set(key, value)
	}
	if binding.Provider.Api == protocol.ConversationModelApiAnthropic && request.Header.Get("anthropic-version") == "" {
		request.Header.Set("anthropic-version", "2023-06-01")
	}
	return request, nil
}
