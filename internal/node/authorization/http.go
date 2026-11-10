package authorization

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"strings"
	"unicode"

	"github.com/egoist/mygo/netproxy"
	"github.com/kaptinlin/jsonpointer"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/http/httpguts"
)

const BodyLimit = 64 * 1024

func HTTPClient() *http.Client {
	return &http.Client{Transport: netproxy.Transport(), CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
}

func Expand(template string, fields map[string]string) (string, error) {
	var result strings.Builder
	for {
		start := strings.IndexByte(template, '{')
		if start < 0 {
			result.WriteString(template)
			return result.String(), nil
		}
		result.WriteString(template[:start])
		template = template[start+1:]
		end := strings.IndexByte(template, '}')
		if end < 0 {
			return "", invalid()
		}
		value, exists := fields[template[:end]]
		if !exists {
			return "", invalid()
		}
		result.WriteString(value)
		template = template[end+1:]
	}
}

func expandJSON(value any, fields map[string]string) (any, error) {
	switch current := value.(type) {
	case string:
		return Expand(current, fields)
	case []any:
		result := make([]any, len(current))
		for index, item := range current {
			value, err := expandJSON(item, fields)
			if err != nil {
				return nil, err
			}
			result[index] = value
		}
		return result, nil
	case map[string]any:
		result := make(map[string]any, len(current))
		for name, item := range current {
			value, err := expandJSON(item, fields)
			if err != nil {
				return nil, err
			}
			result[name] = value
		}
		return result, nil
	default:
		return current, nil
	}
}

func Prepare(ctx context.Context, endpoint protocol.PluginModelAuthEndpoint, fields map[string]string) (*http.Request, error) {
	target, err := url.Parse(endpoint.Url)
	if err != nil || target.Scheme != "https" || target.Host == "" || target.User != nil || target.Fragment != "" {
		return nil, invalid()
	}
	query := target.Query()
	for name, template := range endpoint.Query {
		value, err := Expand(template, fields)
		if err != nil {
			return nil, err
		}
		query.Add(name, value)
	}
	target.RawQuery = query.Encode()
	var body io.Reader
	contentType := ""
	if endpoint.Body != nil {
		switch endpoint.Body.Kind {
		case "form":
			var declaration protocol.PluginModelAuthBodyFormPayload
			if decode(endpoint.Body.Data, &declaration) != nil {
				return nil, invalid()
			}
			form := url.Values{}
			for name, template := range declaration.Fields {
				value, err := Expand(template, fields)
				if err != nil {
					return nil, err
				}
				form.Set(name, value)
			}
			body, contentType = strings.NewReader(form.Encode()), "application/x-www-form-urlencoded"
		case "json":
			var declaration protocol.PluginModelAuthBodyJsonPayload
			var original any
			if decode(endpoint.Body.Data, &declaration) != nil || json.Unmarshal(declaration.Value, &original) != nil {
				return nil, invalid()
			}
			expanded, err := expandJSON(original, fields)
			if err != nil {
				return nil, err
			}
			encoded, err := json.Marshal(expanded)
			if err != nil {
				return nil, invalid()
			}
			body, contentType = bytes.NewReader(encoded), "application/json"
		default:
			return nil, invalid()
		}
	}
	method := string(endpoint.Method)
	if method == "" {
		method = http.MethodPost
	}
	if method != http.MethodGet && method != http.MethodPost {
		return nil, invalid()
	}
	request, err := http.NewRequestWithContext(ctx, method, target.String(), body)
	if err != nil {
		return nil, invalid()
	}
	request.Header.Set("User-Agent", "Sailry/0.1")
	for name, template := range endpoint.Headers {
		value, err := Expand(template, fields)
		if err != nil || !httpguts.ValidHeaderFieldName(name) || !httpguts.ValidHeaderFieldValue(value) {
			return nil, invalid()
		}
		switch strings.ToLower(name) {
		case "host", "connection", "content-length", "transfer-encoding", "proxy-authorization", "upgrade":
			return nil, invalid()
		}
		request.Header.Set(name, value)
	}
	if contentType != "" {
		request.Header.Set("Content-Type", contentType)
	}
	return request, nil
}

func Response(ctx context.Context, client *http.Client, endpoint protocol.PluginModelAuthEndpoint, fields map[string]string, errorDocument bool) (int, any, error) {
	request, err := Prepare(ctx, endpoint, fields)
	if err != nil {
		return 0, nil, err
	}
	response, err := client.Do(request)
	if err != nil {
		if ctx.Err() != nil {
			return 0, nil, cancelled()
		}
		return 0, nil, unavailable()
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, BodyLimit+1))
	if err != nil || len(body) > BodyLimit {
		return 0, nil, invalid()
	}
	if response.StatusCode == 401 || response.StatusCode >= 500 {
		return response.StatusCode, nil, nil
	}
	var document any
	if json.Unmarshal(body, &document) != nil {
		if errorDocument && (response.StatusCode < 200 || response.StatusCode >= 300) {
			return response.StatusCode, nil, nil
		}
		return 0, nil, invalid()
	}
	return response.StatusCode, document, nil
}

func pointer(document any, path string) any {
	value, err := jsonpointer.Value(document, path)
	if err != nil {
		return nil
	}
	return value
}

func text(document any, path string) (string, error) {
	value := pointer(document, path)
	if value == nil {
		return "", nil
	}
	result, ok := value.(string)
	if !ok || result == "" {
		return "", invalid()
	}
	return result, nil
}

func bounded(value string, limit int) bool {
	return value != "" && len(value) <= limit && !strings.ContainsFunc(value, unicode.IsControl)
}

func decode(input, output any) error {
	body, err := json.Marshal(input)
	if err == nil {
		err = json.Unmarshal(body, output)
	}
	return err
}

func invalid() error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "model authorization data is invalid"}
}
func cancelled() error {
	return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "model authorization cancelled"}
}
func expired() error {
	return &protocol.Fault{Code: protocol.ErrorCodeExpired, Message: "authorization attempt expired"}
}
func unavailable() error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "authorization endpoint is unavailable"}
}
func denied() error {
	return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "authorization was rejected"}
}
