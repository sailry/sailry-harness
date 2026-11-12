package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"io"
	"maps"
	"net/http"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type providerDocument struct {
	ready   chan struct{}
	result  protocol.PluginHttpResponse
	err     error
	fetched time.Time
	used    time.Time
}

func (node *Node) providerHTTP(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	if request.Plugin == nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "provider resource requires plugin provenance"}
	}
	var provider protocol.ProviderID
	var expected uint64
	var name string
	var fields map[string]string
	mutation := request.Command.Kind == "execute_provider_http"
	switch input := request.Command.Data.(type) {
	case *protocol.CommandRequestProviderHttpPayload:
		provider, expected, name, fields = input.Provider, input.ExpectedRevision, input.Request, input.Input
	case *protocol.CommandExecuteProviderHttpPayload:
		provider, expected, name, fields = input.Provider, input.ExpectedRevision, input.Request, input.Input
	default:
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "provider resource command is invalid"}
	}
	if err := node.renewProvider(ctx, provider); err != nil {
		return protocol.Output{}, err
	}
	var capture store.ProviderResourceCapture
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		capture, err = store.CaptureProviderResource(ctx, db, node.ID(), request.Plugin.Package, provider, expected, name, mutation)
		return err
	})
	if err != nil {
		return protocol.Output{}, err
	}
	prepared, err := prepareProviderHTTP(ctx, capture, fields)
	if err != nil {
		return protocol.Output{}, err
	}
	fetch := func() (protocol.PluginHttpResponse, error) {
		return plugins.ExecuteHTTP(ctx, prepared, authorization.HTTPClient())
	}
	var value protocol.PluginHttpResponse
	if !mutation && capture.Endpoint.CacheTtlMs != nil {
		body, err := json.Marshal([]any{request.Plugin.Package, name, prepared.URL.String(), prepared.Method, prepared.Headers, prepared.Body, capture.Credential.ID, capture.Credential.Revision})
		if err != nil {
			return protocol.Output{}, err
		}
		value, err = node.providerResourceCached(ctx, blake3.Sum256(body), time.Duration(*capture.Endpoint.CacheTtlMs)*time.Millisecond, fetch)
		if err != nil {
			return protocol.Output{}, err
		}
	} else {
		value, err = fetch()
		if err != nil {
			return protocol.Output{}, err
		}
	}
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error { return store.CheckProviderResource(ctx, db, capture) })
	return protocol.Output{Kind: "plugin_http", Data: value}, err
}

func prepareProviderHTTP(ctx context.Context, capture store.ProviderResourceCapture, input map[string]string) (plugins.HTTPInput, error) {
	endpoint := capture.Endpoint
	if strings.ContainsAny(endpoint.Url, "{}") {
		return plugins.HTTPInput{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "provider resource endpoint must be fixed"}
	}
	for name, value := range endpoint.Headers {
		if strings.ContainsAny(value, "{}") || capture.Provider.Authentication != protocol.AuthenticationPlugin && (strings.EqualFold(name, "authorization") || strings.EqualFold(name, "chatgpt-account-id")) {
			return plugins.HTTPInput{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "provider resource headers must be fixed and cannot replace account headers"}
		}
	}
	fields := map[string]string{}
	for name, value := range input {
		if name == "" || len(name) > 128 || strings.ContainsFunc(name, func(character rune) bool {
			return !(character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' || character >= '0' && character <= '9' || strings.ContainsRune("._-", character))
		}) {
			return plugins.HTTPInput{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "provider resource input name is invalid"}
		}
		fields["input."+name] = value
	}
	request, err := authorization.Prepare(ctx, endpoint, fields)
	if err != nil {
		return plugins.HTTPInput{}, err
	}
	var body *string
	if request.Body != nil {
		content, err := io.ReadAll(io.LimitReader(request.Body, plugins.HTTPMaxBody+1))
		_ = request.Body.Close()
		if err != nil || len(content) > plugins.HTTPMaxBody {
			return plugins.HTTPInput{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "provider resource body exceeds the HTTP budget"}
		}
		text := string(content)
		body = &text
	}
	request.Header.Del("User-Agent")
	for name, value := range endpoint.Headers {
		request.Header.Set(name, value)
	}
	headers := map[string]string{}
	for name := range request.Header {
		headers[name] = request.Header.Get(name)
	}
	prepared, err := plugins.PrepareHTTP(protocol.PluginHttpRequest{Method: request.Method, Url: request.URL.String(), Headers: headers, Body: body, TimeoutMs: 60_000}, nil, nil)
	if err != nil {
		return prepared, err
	}
	grant, auth, err := providers.AccountResourceHeaders(capture.Provider, &capture.Secret, endpoint.Url)
	if err != nil {
		return prepared, err
	}
	for name, values := range auth {
		if prepared.Headers.Get(name) != "" {
			return plugins.HTTPInput{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "provider credential header cannot be overridden"}
		}
		prepared.Headers[name] = values
	}
	prepared.Protection = grant.Secrets()
	if grant.Kind != protocol.AuthenticationPlugin {
		prepared.Protection = nil
		for _, value := range []string{grant.Access, grant.Refresh, grant.IDToken, grant.Github} {
			if value != "" {
				prepared.Protection = append(prepared.Protection, protocol.NewSecret(value))
			}
		}
	}
	return prepared, nil
}

func (node *Node) providerResourceCached(ctx context.Context, key [32]byte, ttl time.Duration, fetch func() (protocol.PluginHttpResponse, error)) (protocol.PluginHttpResponse, error) {
	requested := time.Now()
	for {
		node.auth.mu.Lock()
		document := node.auth.resources[key]
		if document != nil {
			document.used = time.Now()
			if document.ready != nil {
				ready := document.ready
				node.auth.mu.Unlock()
				select {
				case <-ctx.Done():
					return protocol.PluginHttpResponse{}, ctx.Err()
				case <-ready:
				}
				continue
			}
			// A failure belongs to the caller whose context made the request:
			// one who waited behind it fetches again under its own.
			if document.err == nil && (!document.fetched.Before(requested) || document.result.Status >= http.StatusOK && document.result.Status < http.StatusMultipleChoices && time.Since(document.fetched) < ttl) {
				result := document.result
				node.auth.mu.Unlock()
				return copyProviderResponse(result), nil
			}
		} else {
			for len(node.auth.resources) >= 32 {
				var oldest [32]byte
				var used time.Time
				for candidate, value := range node.auth.resources {
					if value.ready == nil && (used.IsZero() || value.used.Before(used)) {
						oldest, used = candidate, value.used
					}
				}
				if used.IsZero() {
					node.auth.mu.Unlock()
					return protocol.PluginHttpResponse{}, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "provider resource cache is busy"}
				}
				delete(node.auth.resources, oldest)
			}
			document = &providerDocument{}
			node.auth.resources[key] = document
		}
		document.ready, document.used = make(chan struct{}), time.Now()
		ready := document.ready
		node.auth.mu.Unlock()
		result, err := fetch()
		node.auth.mu.Lock()
		document.result, document.err, document.fetched, document.ready = copyProviderResponse(result), err, time.Now(), nil
		close(ready)
		node.auth.mu.Unlock()
		return result, err
	}
}

func copyProviderResponse(value protocol.PluginHttpResponse) protocol.PluginHttpResponse {
	value.Headers = maps.Clone(value.Headers)
	for name, entries := range value.Headers {
		value.Headers[name] = slices.Clone(entries)
	}
	return value
}
