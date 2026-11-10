package node

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"regexp"
	"sync"
	"time"

	"github.com/egoist/mygo/netproxy"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const networkPresetURL = "https://sailry.dev/catalogs/network-presets.json"

type networkDirectory struct {
	mu         sync.Mutex
	refreshing chan struct{}
	listing    protocol.NetworkListing
	completed  uint64
	source     string
	transport  http.RoundTripper
}

func (directory *networkDirectory) read(ctx context.Context, refresh bool) (protocol.NetworkListing, error) {
	directory.mu.Lock()
	if !refresh {
		value := directory.listing
		directory.mu.Unlock()
		return value, nil
	}
	if pending := directory.refreshing; pending != nil {
		completed := directory.completed
		directory.mu.Unlock()
		select {
		case <-ctx.Done():
			return protocol.NetworkListing{}, ctx.Err()
		case <-pending:
		}
		directory.mu.Lock()
		if completed == directory.completed {
			directory.mu.Unlock()
			return directory.read(ctx, true)
		}
		value := directory.listing
		directory.mu.Unlock()
		return value, nil
	}
	pending := make(chan struct{})
	directory.refreshing = pending
	directory.mu.Unlock()
	catalog, err := directory.download(ctx)
	directory.mu.Lock()
	if err == nil {
		directory.listing.Presets = catalog.Presets
	}
	if ctx.Err() == nil {
		directory.listing.Unavailable = err != nil
		directory.completed++
	}
	directory.refreshing = nil
	close(pending)
	value := directory.listing
	directory.mu.Unlock()
	if ctx.Err() != nil {
		return protocol.NetworkListing{}, ctx.Err()
	}
	return value, nil
}

func (directory *networkDirectory) download(ctx context.Context) (protocol.NetworkCatalog, error) {
	source := directory.source
	if source == "" {
		source = networkPresetURL
	}
	transport := directory.transport
	if transport == nil {
		transport = netproxy.Transport()
	}
	client := &http.Client{
		Transport: transport, Timeout: 10 * time.Second,
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse },
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, source, nil)
	if err != nil {
		return protocol.NetworkCatalog{}, err
	}
	request.Header.Set("Accept", "application/json")
	response, err := client.Do(request)
	if err != nil {
		return protocol.NetworkCatalog{}, err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 || response.ContentLength > 128*1024 {
		return protocol.NetworkCatalog{}, invalidNetworkCatalog()
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, 128*1024+1))
	if err != nil {
		return protocol.NetworkCatalog{}, err
	}
	return parseNetworkCatalog(ctx, body)
}

var networkPresetID = regexp.MustCompile(`^[a-zA-Z0-9_-]{1,64}$`)

func parseNetworkCatalog(ctx context.Context, body []byte) (protocol.NetworkCatalog, error) {
	var catalog protocol.NetworkCatalog
	if len(body) > 128*1024 || json.Unmarshal(body, &catalog) != nil || catalog.Version != 1 || len(catalog.Presets) > 32 {
		return catalog, invalidNetworkCatalog()
	}
	ids := map[string]bool{}
	for _, preset := range catalog.Presets {
		if !networkPresetID.MatchString(preset.ID) || ids[preset.ID] || !preset.Label.Valid() {
			return catalog, invalidNetworkCatalog()
		}
		ids[preset.ID] = true
		if _, err := link.NetworkRelayURLs(ctx, preset.Selection); err != nil {
			return catalog, invalidNetworkCatalog()
		}
	}
	return catalog, nil
}

func invalidNetworkCatalog() error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "network preset catalog is unavailable"}
}
