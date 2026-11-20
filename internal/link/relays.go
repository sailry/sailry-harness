package link

import (
	"context"
	"encoding/json"
	"sort"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const relayProbeTimeout = 20 * time.Second

func (link *Link) NetworkSelection() *protocol.NetworkSelection {
	link.mu.RLock()
	defer link.mu.RUnlock()
	if link.selection == nil {
		return nil
	}
	selected := cloneNetworkSelection(*link.selection)
	return &selected
}

func (link *Link) SelectNetwork(ctx context.Context, selection protocol.NetworkSelection) error {
	next, err := NetworkRelayURLs(ctx, selection)
	if err != nil {
		return err
	}
	scoped, release, err := link.acquireRelayUpdate(ctx)
	if err != nil {
		return err
	}
	defer release()
	if err := scoped.Err(); err != nil {
		return linkNetwork(err)
	}
	link.mu.RLock()
	managed := link.selection != nil
	link.mu.RUnlock()
	if !managed {
		return invalidLink("direct-only endpoint has no relay transport")
	}
	probe, expire := link.timeoutContext(scoped, relayProbeTimeout)
	err = link.endpoint.SetRelays(probe, next)
	expire()
	if err != nil {
		return linkNetwork(err)
	}
	link.mu.Lock()
	selected := cloneNetworkSelection(selection)
	link.selection = &selected
	link.mu.Unlock()
	return nil
}

func (link *Link) acquireRelayUpdate(ctx context.Context) (context.Context, func(), error) {
	scoped, cancel := link.operation(ctx, false)
	link.mu.Lock()
	if link.relayUpdate == nil {
		link.relayUpdate = make(chan struct{}, 1)
		link.relayUpdate <- struct{}{}
	}
	update := link.relayUpdate
	link.mu.Unlock()
	select {
	case <-scoped.Done():
		err := linkNetwork(scoped.Err())
		cancel()
		return nil, nil, err
	case <-update:
		return scoped, func() {
			update <- struct{}{}
			cancel()
		}, nil
	}
}

func CandidateAddress(address EndpointAddress, selection protocol.NetworkSelection) (EndpointAddress, error) {
	relays, err := NetworkRelayURLs(context.Background(), selection)
	if err != nil {
		return EndpointAddress{}, err
	}
	dial, err := libp2pDialAddress(address)
	if err != nil {
		return EndpointAddress{}, err
	}
	if _, err := libp2pAddressInfo(dial); err != nil {
		return EndpointAddress{}, err
	}
	candidate := cloneAddress(address)
	for _, relay := range relays {
		hint, err := json.Marshal(map[string]string{"Libp2p": relay + "/p2p-circuit"})
		if err != nil {
			return EndpointAddress{}, invalidLink("invalid Libp2p relay address")
		}
		candidate.Addrs = append(candidate.Addrs, hint)
	}
	return mergeAddress(candidate, EndpointAddress{ID: address.ID}), nil
}

// NetworkRelayURLs retains its historical name; active relay selections use
// Libp2p multiaddrs. Direct returns a non-nil empty list.
func NetworkRelayURLs(_ context.Context, selection protocol.NetworkSelection) ([]string, error) {
	switch selection.Kind {
	case "direct":
		if selection.Data != nil {
			return nil, invalidLink("invalid direct network selection")
		}
		return []string{}, nil
	case "iroh":
		return nil, invalidLink("Iroh is no longer supported; select Direct or configure Libp2p relays")
	case "custom":
		relays := customRelayAddrs(selection.Data)
		if len(relays) == 0 {
			return nil, invalidLink("expected at least one Libp2p relay multiaddr")
		}
		for _, value := range relays {
			lower := strings.ToLower(value)
			if strings.HasPrefix(lower, "https://") || strings.HasPrefix(lower, "http://") {
				return nil, invalidLink("Iroh relay URLs are no longer supported; use a Libp2p relay multiaddr")
			}
		}
		specs, err := parseLibp2pRelays(relays)
		if err != nil {
			return nil, err
		}
		result := make([]string, 0, len(specs))
		for _, spec := range specs {
			result = append(result, spec.circuit[:len(spec.circuit)-1].String())
		}
		sort.Strings(result)
		return result, nil
	default:
		return nil, invalidLink("invalid relay selection")
	}
}

func customRelayAddrs(data any) []string {
	switch payload := data.(type) {
	case protocol.NetworkSelectionCustomPayload:
		return payload.Relays
	case *protocol.NetworkSelectionCustomPayload:
		if payload != nil {
			return payload.Relays
		}
	}
	return nil
}

func libp2pDialAddress(address EndpointAddress) (EndpointAddress, error) {
	result := EndpointAddress{ID: address.ID}
	var direct []json.RawMessage
	legacy := false
	for _, hint := range address.Addrs {
		var transport map[string]json.RawMessage
		if json.Unmarshal(hint, &transport) != nil {
			continue
		}
		legacy = legacy || transport["Relay"] != nil || transport["Ip"] != nil
		encoded := transport["Libp2p"]
		if encoded == nil {
			continue
		}
		var route string
		if json.Unmarshal(encoded, &route) == nil && strings.HasSuffix(route, "/p2p-circuit") {
			result.Addrs = append(result.Addrs, hint)
		} else {
			direct = append(direct, hint)
		}
	}
	// Recovery can add a new circuit to a full set of advertised hints.
	result.Addrs = append(result.Addrs, direct...)
	if len(result.Addrs) != 0 {
		result.Addrs = result.Addrs[:min(len(result.Addrs), libp2pMaxHints)]
		return result, nil
	}
	message := "peer has no Libp2p address; pair again or import a Libp2p address"
	if legacy {
		message = "peer has only Iroh address hints; pair again or import a Libp2p address"
	}
	return EndpointAddress{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}
}

func cloneNetworkSelection(selection protocol.NetworkSelection) protocol.NetworkSelection {
	switch payload := selection.Data.(type) {
	case protocol.NetworkSelectionCustomPayload:
		selection.Data = protocol.NetworkSelectionCustomPayload{Relays: append([]string{}, payload.Relays...)}
	case *protocol.NetworkSelectionCustomPayload:
		if payload != nil {
			selection.Data = protocol.NetworkSelectionCustomPayload{Relays: append([]string{}, payload.Relays...)}
		}
	}
	return selection
}
