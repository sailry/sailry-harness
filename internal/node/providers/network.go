// Package providers owns provider metadata and explicitly requested HTTP work.
package providers

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/egoist/mygo/netproxy"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Work struct {
	Command       protocol.Command
	Provider      *protocol.ConversationProvider
	Secret        *protocol.Secret
	ETag          *string
	Directory     *Directory
	Client        *http.Client
	ScoreRevision uint64
}
type Prepared struct {
	Provider      *protocol.ConversationProvider
	Output        *protocol.Output
	Catalog       *CatalogDownload
	Scores        *ScoresDownload
	ScoreRevision uint64
}

func Run(ctx context.Context, work Work) (*Prepared, error) {
	result := &Prepared{ScoreRevision: work.ScoreRevision, Provider: work.Provider}
	switch work.Command.Kind {
	case "refresh_model_catalog":
		catalog, err := downloadCatalog(ctx)
		if err != nil {
			catalog = &CatalogDownload{Fault: asFault(err)}
		}
		result.Catalog = catalog
		scores, err := downloadScores(ctx, work.ETag)
		if err != nil {
			scores = &ScoresDownload{Fault: asFault(err)}
		}
		result.Scores = scores
		if ctx.Err() != nil {
			return nil, cancelled("model catalog refresh cancelled")
		}
	case "discover_models":
		if work.Provider == nil {
			return nil, invalid("provider discovery capture is missing")
		}
		catalog, err := discover(ctx, *work.Provider, work.Secret, work.Client)
		if err != nil {
			return nil, err
		}
		output := protocol.Output{Kind: "discovered_models", Data: catalog}
		result.Output = &output
	case "list_channel_presets":
		directory := work.Directory
		if directory == nil {
			directory = &Directory{}
		}
		listing, err := directory.Read(ctx)
		if err != nil {
			return nil, err
		}
		output := protocol.Output{Kind: "channel_presets", Data: listing}
		result.Output = &output
	default:
		return nil, invalid("provider network command is unsupported")
	}
	return result, nil
}
func invalid(message string) *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}
func unavailable(message string) *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}
}
func cancelled(message string) *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: message}
}
func asFault(err error) *protocol.Fault {
	if value, ok := err.(*protocol.Fault); ok {
		return value
	}
	return unavailable("provider metadata request failed")
}
func httpClient(timeout time.Duration) *http.Client {
	return &http.Client{Transport: netproxy.Transport(), Timeout: timeout, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
}
func readJSON(response *http.Response, limit int64) ([]byte, error) {
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return nil, unavailable("provider metadata request was rejected")
	}
	if response.ContentLength > limit {
		return nil, unavailable("provider metadata exceeds its size limit")
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, limit+1))
	if err != nil {
		return nil, unavailable("provider metadata response failed")
	}
	if int64(len(raw)) > limit {
		return nil, unavailable("provider metadata exceeds its size limit")
	}
	return raw, nil
}
func Endpoint(value string) (*url.URL, error) {
	value = strings.TrimSpace(value)
	if value == "" || len(value) > 2048 {
		return nil, invalid("provider endpoint is empty or too long")
	}
	if !strings.Contains(value, "://") {
		if strings.HasPrefix(value, "/") {
			return nil, invalid("provider endpoint must include a host")
		}
		value = "https://" + value
	}
	endpoint, err := url.Parse(value)
	if err != nil || endpoint.Hostname() == "" || (endpoint.Scheme != "http" && endpoint.Scheme != "https") || endpoint.User != nil || endpoint.RawQuery != "" || endpoint.Fragment != "" {
		return nil, invalid("provider endpoint must be an HTTP URL without credentials, query or fragment")
	}
	return endpoint, nil
}
func headers(provider protocol.ConversationProvider, secret *protocol.Secret) (http.Header, error) {
	if provider.Authentication != protocol.AuthenticationApiKey {
		_, headers, err := AccountHeaders(provider, secret)
		return headers, err
	}
	result := make(http.Header)
	for name, value := range APIKeyHeaderDefaults(provider.Api) {
		result.Set(name, value)
	}
	credentialHeader := "authorization"
	switch provider.Api {
	case protocol.ConversationModelApiAnthropic:
		credentialHeader = "x-api-key"
	case protocol.ConversationModelApiGemini:
		credentialHeader = "x-goog-api-key"
	case protocol.ConversationModelApiAzureAi, protocol.ConversationModelApiAzureOpenAi:
		credentialHeader = "api-key"
	}
	var injection *protocol.PluginModelAuthInjection
	if provider.Binding != nil && *provider.Binding != nil && (**provider.Binding).KeyInjection != nil {
		injection = (**provider.Binding).KeyInjection
		if err := validateInjection(*injection); err != nil {
			return nil, err
		}
		credentialHeader = injection.Header
	}
	if secret != nil {
		value := secret.Expose()
		if injection != nil {
			value = strings.ReplaceAll(injection.Template, "{key}", value)
		} else if credentialHeader == "authorization" {
			value = "Bearer " + value
		}
		if strings.ContainsAny(value, "\r\n") {
			return nil, invalid("provider credential is invalid")
		}
		result.Set(credentialHeader, value)
	}
	for name, value := range provider.Headers {
		if strings.EqualFold(name, credentialHeader) || ReservedHeader(name) {
			continue
		}
		result.Set(name, value)
	}
	return result, nil
}
func object(raw json.RawMessage) map[string]json.RawMessage {
	var result map[string]json.RawMessage
	json.Unmarshal(raw, &result)
	return result
}
