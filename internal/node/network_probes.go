package node

import (
	"context"
	"slices"
	"strings"
	"sync"
	"time"

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
	mu      sync.Mutex
	cache   []networkProbeEntry
	permits chan struct{}
	now     func() time.Time
}

func (probes *networkProbes) read(ctx context.Context, selection protocol.NetworkSelection) (protocol.NetworkLatency, error) {
	relays, err := link.NetworkRelayURLs(ctx, selection)
	if err != nil {
		return protocol.NetworkLatency{}, err
	}
	slices.Sort(relays)
	relays = slices.Compact(relays)
	key := strings.Join(relays, "\n")
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
	owner, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	results := make(chan *uint64, len(relays))
	for _, address := range relays {
		go func() {
			select {
			case <-owner.Done():
				results <- nil
				return
			case probes.permits <- struct{}{}:
			}
			defer func() { <-probes.permits }()
			rtt, err := link.ProbeRelay(owner, address)
			if err != nil {
				results <- nil
				return
			}
			elapsed := uint64(max(0, rtt.Milliseconds()))
			results <- &elapsed
		}()
	}
	for range relays {
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
