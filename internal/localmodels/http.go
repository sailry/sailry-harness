package localmodels

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/localmodels/assets"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	pageSize      = 20
	metadataBytes = 4 * 1024 * 1024
	smallBytes    = 32 * 1024 * 1024
)

// Hubs performs anonymous catalog requests over the Node's outbound transport.
type Hubs struct {
	// Transport carries the Node's proxy settings.
	Transport http.RoundTripper

	mu       sync.Mutex
	fixtures map[protocol.ModelsHub]*url.URL
}

// Fixture redirects one Hub to a loopback server. Tests use it; production
// never calls it.
func (hubs *Hubs) Fixture(hub protocol.ModelsHub, endpoint string) error {
	address, err := url.Parse(endpoint)
	if err != nil {
		return invalid("invalid model catalog fixture URL")
	}
	ip := net.ParseIP(address.Hostname())
	if address.Scheme != "http" && address.Scheme != "https" || ip == nil || !ip.IsLoopback() ||
		address.User != nil || address.RawQuery != "" || address.Fragment != "" {
		return invalid("model catalog fixture requires a loopback URL without credentials")
	}
	hubs.mu.Lock()
	defer hubs.mu.Unlock()
	if hubs.fixtures == nil {
		hubs.fixtures = map[protocol.ModelsHub]*url.URL{}
	}
	hubs.fixtures[hub] = address
	return nil
}

// catalog is one Hub endpoint during a request.
type catalog struct {
	client  *http.Client
	base    *url.URL
	fixture bool
}

func (hubs *Hubs) open(hub protocol.ModelsHub, mirror *string) (*catalog, error) {
	hubs.mu.Lock()
	fixture := hubs.fixtures[hub]
	hubs.mu.Unlock()
	if fixture != nil {
		// A loopback fixture bypasses the proxy and may use plain HTTP.
		return &catalog{client: redirecting(&http.Transport{}, false), base: fixture, fixture: true}, nil
	}
	address := "https://modelscope.cn/"
	if hub == protocol.ModelsHubHuggingFace {
		address = "https://huggingface.co/"
		if mirror != nil {
			address = *mirror
		}
	}
	base, err := url.Parse(address)
	if err != nil {
		return nil, invalid("invalid model Hub URL")
	}
	if !assets.Origin(base) {
		return nil, invalid("model Hub requires HTTPS without credentials")
	}
	return &catalog{client: redirecting(hubs.Transport, true), base: base}, nil
}

func redirecting(transport http.RoundTripper, secure bool) *http.Client {
	return &http.Client{Transport: transport, CheckRedirect: func(request *http.Request, via []*http.Request) error {
		if secure && request.URL.Scheme != "https" {
			return errors.New("redirect leaves HTTPS")
		}
		if len(via) > 5 {
			return errors.New("stopped after 5 redirects")
		}
		return nil
	}}
}

// url appends percent-encoded segments to the Hub base.
func (hub *catalog) url(segments ...string) *url.URL {
	address := *hub.base
	raw := strings.TrimSuffix(address.EscapedPath(), "/")
	plain := strings.TrimSuffix(address.Path, "/")
	for _, segment := range segments {
		raw += "/" + url.PathEscape(segment)
		plain += "/" + segment
	}
	address.Path, address.RawPath = plain, raw
	return &address
}

func (hub *catalog) repository(prefix []string, repository string, suffix ...string) *url.URL {
	segments := append(append(append([]string{}, prefix...), strings.Split(repository, "/")...), suffix...)
	return hub.url(segments...)
}

// file is the download address of one repository file at its pinned revision.
func (hub *catalog) file(file *file) (*url.URL, error) {
	base := *hub.base
	if hub.fixture {
		base.Scheme = "https"
	}
	origin := base.String()
	address, err := assets.URL(protocol.ModelsAsset{Source: file.source, Path: file.path, Name: "metadata.json", Bytes: file.bytes, Sha256: file.sha256}, &origin)
	if err != nil {
		return nil, invalid("invalid pinned model asset URL")
	}
	if hub.fixture {
		fixture := *hub.base
		fixture.Path, fixture.RawPath, fixture.RawQuery = address.Path, address.RawPath, address.RawQuery
		return &fixture, nil
	}
	return address, nil
}

func requestCancelled() error {
	return newFault(protocol.ErrorCodeCancelled, "model catalog request cancelled")
}

func requestFailed() error {
	return newFault(protocol.ErrorCodeUnavailable, "model catalog request failed")
}

// json reads one bounded metadata document within a minute.
func (hub *catalog) json(ctx context.Context, address *url.URL) (any, http.Header, error) {
	limited, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()
	body, headers, err := hub.get(limited, address, metadataBytes)
	if err != nil {
		return nil, nil, err
	}
	value, err := decode(body)
	if err != nil {
		return nil, nil, invalid("model catalog returned invalid JSON")
	}
	return value, headers, nil
}

// decode keeps numbers exact so sizes survive as unsigned integers.
func decode(body []byte) (any, error) {
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.UseNumber()
	var value any
	if err := decoder.Decode(&value); err != nil {
		return nil, err
	}
	if _, err := decoder.Token(); err != io.EOF {
		return nil, errors.New("trailing data")
	}
	return value, nil
}

// get reads a response of at most limit bytes. Thirty seconds without
// response bytes fail the request.
func (hub *catalog) get(ctx context.Context, address *url.URL, limit int) ([]byte, http.Header, error) {
	scoped, cancel := context.WithCancel(ctx)
	defer cancel()
	idle := time.AfterFunc(30*time.Second, cancel)
	defer idle.Stop()
	failure := func() error {
		if ctx.Err() != nil {
			if errors.Is(ctx.Err(), context.DeadlineExceeded) {
				return requestFailed()
			}
			return requestCancelled()
		}
		return requestFailed()
	}
	request, err := http.NewRequestWithContext(scoped, http.MethodGet, address.String(), nil)
	if err != nil {
		return nil, nil, requestFailed()
	}
	request.Header.Set("User-Agent", "Sailry model catalog")
	response, err := hub.client.Do(request)
	if err != nil {
		return nil, nil, failure()
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode > 299 {
		switch response.StatusCode {
		case http.StatusNotFound:
			return nil, nil, newFault(protocol.ErrorCodeNotFound, "model repository or revision was not found")
		case http.StatusUnauthorized, http.StatusForbidden:
			return nil, nil, newFault(protocol.ErrorCodePermissionDenied, "model repository requires authentication")
		}
		return nil, nil, requestFailed()
	}
	oversized := invalid("model catalog response exceeds its size limit")
	if response.ContentLength > int64(limit) {
		return nil, nil, oversized
	}
	var body []byte
	buffer := make([]byte, 64*1024)
	for {
		count, err := response.Body.Read(buffer)
		if count > 0 {
			idle.Reset(30 * time.Second)
			if len(body)+count > limit {
				return nil, nil, oversized
			}
			body = append(body, buffer[:count]...)
		}
		if err == io.EOF {
			return body, response.Header, nil
		}
		if err != nil {
			return nil, nil, failure()
		}
	}
}

// Document accessors over decoded metadata. Absent and mistyped values are
// both reported as missing.

func field(value any, key string) (any, bool) {
	object, ok := value.(map[string]any)
	if !ok {
		return nil, false
	}
	found, ok := object[key]
	return found, ok
}

func text(value any, key string) (string, bool) {
	found, _ := field(value, key)
	result, ok := found.(string)
	return result, ok
}

func flag(value any, key string) (bool, bool) {
	found, _ := field(value, key)
	result, ok := found.(bool)
	return result, ok
}

func unsigned(value any) (uint64, bool) {
	number, ok := value.(json.Number)
	if !ok {
		return 0, false
	}
	result, err := strconv.ParseUint(string(number), 10, 64)
	return result, err == nil
}

func count(value any, key string) (uint64, bool) {
	found, _ := field(value, key)
	return unsigned(found)
}

func list(value any, key string) ([]any, bool) {
	found, _ := field(value, key)
	result, ok := found.([]any)
	return result, ok
}
