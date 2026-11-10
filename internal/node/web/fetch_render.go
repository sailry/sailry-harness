package web

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net"
	"net/http"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type Renderer struct {
	Endpoint  string
	Key       protocol.Secret
	Transport http.RoundTripper
}

func (renderer *Renderer) render(ctx context.Context, fetcher *Fetcher, direct webDocument) (webDocument, error) {
	deadline, ok := ctx.Deadline()
	if !ok {
		return webDocument{}, webUnavailable("web fetch deadline exceeded")
	}
	remaining := time.Until(deadline)
	if remaining <= 0 {
		return webDocument{}, webUnavailable("web fetch deadline exceeded")
	}
	body, err := json.Marshal(map[string]any{"urls": []string{direct.url}, "format": "markdown", "ttl": 0, "per_url_timeout_ms": min(max(remaining.Milliseconds(), 1), 110000)})
	if err != nil {
		return webDocument{}, err
	}
	endpoint := renderer.Endpoint
	if endpoint == "" {
		endpoint = "https://api.fetch.tinyfish.ai"
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, bytes.NewReader(body))
	if err != nil {
		return webDocument{}, webInvalid("search request is invalid")
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("User-Agent", "Sailry/1")
	request.Header.Set("X-API-Key", renderer.Key.Expose())
	transport := renderer.Transport
	if transport == nil {
		native := &http.Transport{Proxy: systemProxy, DialContext: (&net.Dialer{Timeout: 5 * time.Second}).DialContext, TLSHandshakeTimeout: 5 * time.Second, ForceAttemptHTTP2: true}
		defer native.CloseIdleConnections()
		transport = native
	}
	response, err := transport.RoundTrip(request)
	if err != nil {
		return webDocument{}, webUnavailable("search service is unreachable")
	}
	defer response.Body.Close()
	if response.StatusCode == 401 || response.StatusCode == 403 {
		return webDocument{}, webUnavailable("web rendering authentication failed")
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return webDocument{}, webUnavailable("web rendering failed")
	}
	if response.ContentLength > webBodyLimit {
		return webDocument{}, webInvalid("search service response is too large")
	}
	value, err := io.ReadAll(io.LimitReader(response.Body, webBodyLimit+1))
	if err != nil {
		return webDocument{}, webUnavailable("search response was interrupted")
	}
	if len(value) > webBodyLimit {
		return webDocument{}, webInvalid("search service response is too large")
	}
	var parsed struct {
		Results []json.RawMessage `json:"results"`
		Errors  []json.RawMessage `json:"errors"`
	}
	if err = json.Unmarshal(value, &parsed); err != nil || parsed.Results == nil || parsed.Errors == nil {
		return webDocument{}, webUnavailable("rendered web response is invalid")
	}
	type targetError struct {
		URL   string `json:"url"`
		Error string `json:"error"`
	}
	var first *targetError
	for _, value := range parsed.Errors {
		var target targetError
		if json.Unmarshal(value, &target) != nil {
			return webDocument{}, webUnavailable("rendered web response is invalid")
		}
		if first == nil {
			first = &target
		}
		if target.URL == direct.url {
			return webDocument{}, renderTargetError(target.Error)
		}
	}
	if len(parsed.Results) == 0 {
		if first != nil {
			return webDocument{}, renderTargetError(first.Error)
		}
		return webDocument{}, webUnavailable("rendered web page has no content")
	}
	if len(parsed.Results) != 1 {
		return webDocument{}, webUnavailable("rendered web response is invalid")
	}
	var result struct {
		URL      string  `json:"url"`
		FinalURL *string `json:"final_url"`
		Text     *string `json:"text"`
		Title    *string `json:"title"`
	}
	if err = json.Unmarshal(parsed.Results[0], &result); err != nil {
		return webDocument{}, webUnavailable("rendered web response is invalid")
	}
	if result.URL != direct.url {
		return webDocument{}, webUnavailable("rendered web response does not match the requested page")
	}
	if result.FinalURL == nil {
		return webDocument{}, webUnavailable("rendered web response has no final URL")
	}
	finalURL, err := publicURL(*result.FinalURL)
	if err != nil {
		return webDocument{}, err
	}
	if result.Text == nil {
		return webDocument{}, webUnavailable("rendered web response has no page content")
	}
	if len(*result.Text) > webBodyLimit {
		return webDocument{}, webUnavailable("web content exceeds the size limit")
	}
	if strings.TrimSpace(*result.Text) == "" {
		return webDocument{}, webUnavailable("rendered web page has no content")
	}
	validation, err := http.NewRequestWithContext(ctx, http.MethodGet, finalURL.String(), nil)
	if err != nil {
		return webDocument{}, err
	}
	proxy, err := fetcher.options.Proxy(validation)
	if err != nil {
		return webDocument{}, err
	}
	if err = fetcher.validateDestination(ctx, finalURL, proxy != nil); err != nil {
		return webDocument{}, err
	}
	title := direct.title
	if result.Title != nil && strings.TrimSpace(*result.Title) != "" {
		title = result.Title
	}
	return webDocument{url: finalURL.String(), status: direct.status, contentType: "text/markdown", title: title, content: *result.Text, fetched: fetcher.options.Now()}, nil
}

func renderTargetError(kind string) error {
	switch kind {
	case "invalid_url", "invalid_redirect_url":
		return addressDenied()
	case "content_too_large":
		return webUnavailable("web content exceeds the size limit")
	case "timeout":
		return webUnavailable("rendered web page timed out")
	case "login_required", "bot_blocked":
		return webUnavailable("rendered web page is not publicly available")
	case "page_not_found":
		return webFault(protocol.ErrorCodeNotFound, "rendered web page was not found")
	case "empty_content":
		return webUnavailable("rendered web page has no content")
	default:
		return webUnavailable("web rendering failed")
	}
}
