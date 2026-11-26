package model

import (
	"context"
	"errors"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"charm.land/fantasy"
	"golang.org/x/net/http2"
)

// largeHTTP1BodyBytes is the request body size above which requests prefer
// HTTP/1.1 directly to avoid HTTP/2 per-stream flow-control bottlenecks (such
// as reverse proxies advertising a 64 KiB INITIAL_WINDOW_SIZE) on large posts.
const largeHTTP1BodyBytes = 256 * 1024

// http1OriginTTL bounds how long a confirmed HTTP/1.1 protocol recovery stays
// active for an endpoint origin across turns, subagents, and compactions.
const (
	http1OriginTTL        = 30 * time.Minute
	maxHTTP1OriginEntries = 256
)

type http1OriginMemory struct {
	mu      sync.Mutex
	entries map[string]time.Time
}

var defaultHTTP1Origins = &http1OriginMemory{}

func endpointOrigin(raw string) string {
	parsed, err := url.Parse(strings.TrimSpace(raw))
	if err != nil || parsed.Host == "" {
		return ""
	}
	scheme := strings.ToLower(parsed.Scheme)
	if scheme != "http" && scheme != "https" {
		return ""
	}
	return scheme + "://" + strings.ToLower(parsed.Host)
}

func (model *sdkModel) origin() string {
	if model == nil {
		return ""
	}
	return endpointOrigin(model.config.Endpoint)
}

func (memory *http1OriginMemory) preferred(origin string, now time.Time) bool {
	if origin == "" {
		return false
	}
	memory.mu.Lock()
	defer memory.mu.Unlock()
	expires, ok := memory.entries[origin]
	if !ok {
		return false
	}
	if !expires.After(now) {
		delete(memory.entries, origin)
		return false
	}
	return true
}

func (memory *http1OriginMemory) remember(origin string, now time.Time) {
	if origin == "" {
		return
	}
	memory.mu.Lock()
	defer memory.mu.Unlock()
	if memory.entries == nil {
		memory.entries = map[string]time.Time{}
	}
	if len(memory.entries) >= maxHTTP1OriginEntries {
		var oldestKey string
		var oldestTime time.Time
		for key, expires := range memory.entries {
			if !expires.After(now) {
				delete(memory.entries, key)
				continue
			}
			if oldestKey == "" || expires.Before(oldestTime) {
				oldestKey, oldestTime = key, expires
			}
		}
		if len(memory.entries) >= maxHTTP1OriginEntries && oldestKey != "" {
			delete(memory.entries, oldestKey)
		}
	}
	memory.entries[origin] = now.Add(http1OriginTTL)
}

func (memory *http1OriginMemory) forget(origin string) {
	if origin == "" {
		return
	}
	memory.mu.Lock()
	delete(memory.entries, origin)
	memory.mu.Unlock()
}

type largeHTTP1State struct {
	skip bool
	used atomic.Bool
}

type largeHTTP1Key struct{}

func withLargeHTTP1State(ctx context.Context, skip bool) (context.Context, *largeHTTP1State) {
	state := &largeHTTP1State{skip: skip}
	return context.WithValue(ctx, largeHTTP1Key{}, state), state
}

func largeHTTP1Skipped(ctx context.Context) bool {
	state, ok := ctx.Value(largeHTTP1Key{}).(*largeHTTP1State)
	return ok && state.skip
}

func markLargeHTTP1Used(ctx context.Context) {
	if state, ok := ctx.Value(largeHTTP1Key{}).(*largeHTTP1State); ok {
		state.used.Store(true)
	}
}

func http1ProtocolRejected(status *uint16) bool {
	return status != nil && (*status == http.StatusUpgradeRequired || *status == http.StatusHTTPVersionNotSupported)
}

func http2TransportCause(err error) error {
	var provider *fantasy.ProviderError
	if errors.As(err, &provider) {
		if len(provider.ResponseBody) > 0 {
			return nil
		}
		return provider.Cause
	}
	return err
}

func http2ProtocolFailure(err error) bool {
	// Go's bundled HTTP/2 StreamError supports errors.As into x/net/http2.
	stream, ok := errors.AsType[http2.StreamError](http2TransportCause(err))
	return ok && (stream.Code == http2.ErrCodeProtocol || stream.Code == http2.ErrCodeHTTP11Required)
}

func http2Interrupted(err error) bool {
	err = http2TransportCause(err)
	if _, ok := errors.AsType[http2.StreamError](err); ok {
		return true
	}
	// Go's HTTP/2 transport creates a fresh, unexported plain error when its
	// connection health check fails. Match the cause, not a provider's prose.
	for ; err != nil; err = errors.Unwrap(err) {
		if err.Error() == "http2: client connection lost" {
			return true
		}
	}
	return false
}

// Reuse connections when the captured model selects HTTP/1.1. Other
// transports keep their own proxy and TLS configuration.
var modelHTTP1Transport = sync.OnceValue(func() *http.Transport { return http1Transport(modelTransport()) })

func http1Transport(base *http.Transport) *http.Transport {
	fallback := base.Clone()
	fallback.Protocols = &http.Protocols{}
	fallback.Protocols.SetHTTP1(true)
	if fallback.TLSClientConfig != nil {
		// Clone retains the ALPN offers installed when the original transport negotiated h2.
		fallback.TLSClientConfig.NextProtos = []string{"http/1.1"}
	}
	return fallback
}

func (model *sdkModel) http1Fallback() (*sdkModel, func()) {
	transport := model.client.Transport
	vertex, cloud := transport.(vertexTransport)
	if cloud {
		transport = vertex.base
	}
	claude, oauth := transport.(claudeTransport)
	if oauth {
		transport = claude.base
	}
	response, ok := transport.(responseTransport)
	if !ok {
		return nil, nil
	}
	base, ok := response.base.(*http.Transport)
	if !ok {
		return nil, nil
	}
	var fallback *http.Transport
	release := func() {}
	if base == modelTransport() {
		fallback = modelHTTP1Transport()
	} else {
		fallback = http1Transport(base)
		release = fallback.CloseIdleConnections
	}
	response.base = fallback
	response.http1 = nil
	transport = response
	if oauth {
		claude.base = transport
		transport = claude
	}
	if cloud {
		vertex.base = transport
		transport = vertex
	}
	client := *model.client
	client.Transport = transport
	return &sdkModel{config: model.config, client: &client}, release
}
