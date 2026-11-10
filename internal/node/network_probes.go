package node

import (
	"context"
	"io"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/netproxy"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type networkProbeSlot struct {
	gate     chan struct{}
	measured time.Time
	millis   *uint64
}

type networkProbeEntry struct {
	key  string
	slot *networkProbeSlot
}

type networkProbes struct {
	mu        sync.Mutex
	cache     []networkProbeEntry
	permits   chan struct{}
	transport http.RoundTripper
	now       func() time.Time
}

func (probes *networkProbes) read(ctx context.Context, selection protocol.NetworkSelection) (protocol.NetworkLatency, error) {
	urls, err := link.NetworkRelayURLs(ctx, selection)
	if err != nil {
		return protocol.NetworkLatency{}, err
	}
	slices.Sort(urls)
	urls = slices.Compact(urls)
	key := strings.Join(urls, "\n")
	probes.mu.Lock()
	var slot *networkProbeSlot
	for index, entry := range probes.cache {
		if entry.key == key {
			slot = entry.slot
			probes.cache = slices.Delete(probes.cache, index, index+1)
			break
		}
	}
	if slot == nil {
		slot = &networkProbeSlot{gate: make(chan struct{}, 1)}
	}
	if len(probes.cache) >= 32 {
		probes.cache = probes.cache[1:]
	}
	probes.cache = append(probes.cache, networkProbeEntry{key: key, slot: slot})
	probes.mu.Unlock()
	select {
	case <-ctx.Done():
		return protocol.NetworkLatency{}, ctx.Err()
	case slot.gate <- struct{}{}:
	}
	defer func() { <-slot.gate }()
	value := protocol.NetworkLatency{Selection: selection}
	if !slot.measured.IsZero() && probes.now().Sub(slot.measured) < 30*time.Second {
		value.Millis = slot.millis
		return value, nil
	}
	transport := probes.transport
	if transport == nil {
		transport = netproxy.Transport()
	}
	client := &http.Client{
		Transport: transport, Timeout: 3 * time.Second,
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse },
	}
	owner, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	results := make(chan *uint64, len(urls))
	for _, address := range urls {
		go func() {
			select {
			case <-owner.Done():
				results <- nil
				return
			case probes.permits <- struct{}{}:
			}
			defer func() { <-probes.permits }()
			base, err := url.Parse(address)
			if err != nil {
				results <- nil
				return
			}
			target := base.ResolveReference(&url.URL{Path: "/ping"})
			request, err := http.NewRequestWithContext(owner, http.MethodGet, target.String(), nil)
			if err != nil {
				results <- nil
				return
			}
			request.Close = true
			started := probes.now()
			response, err := client.Do(request)
			if err != nil {
				results <- nil
				return
			}
			elapsed := uint64(max(0, probes.now().Sub(started).Milliseconds()))
			_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, 8*1024))
			_ = response.Body.Close()
			if response.StatusCode < 200 || response.StatusCode >= 300 {
				results <- nil
				return
			}
			results <- &elapsed
		}()
	}
	for range urls {
		select {
		case <-owner.Done():
			if ctx.Err() != nil {
				return value, ctx.Err()
			}
			slot.measured, slot.millis = probes.now(), value.Millis
			return value, nil
		case millis := <-results:
			if millis != nil && (value.Millis == nil || *millis < *value.Millis) {
				value.Millis = millis
			}
		}
	}
	slot.measured, slot.millis = probes.now(), value.Millis
	return value, nil
}
