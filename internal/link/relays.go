package link

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"net/url"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/idna"
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
	scoped, cancel := link.operation(ctx, false)
	defer cancel()
	link.mu.Lock()
	if link.relayUpdate == nil {
		link.relayUpdate = make(chan struct{}, 1)
		link.relayUpdate <- struct{}{}
	}
	update := link.relayUpdate
	link.mu.Unlock()
	select {
	case <-scoped.Done():
		return linkNetwork(scoped.Err())
	case <-update:
	}
	defer func() { update <- struct{}{} }()
	if err := scoped.Err(); err != nil {
		return linkNetwork(err)
	}
	link.mu.RLock()
	previous := append([]string{}, link.relayURLs...)
	direct := link.selection == nil
	link.mu.RUnlock()
	if direct {
		return invalidLink("direct-only endpoint has no relay transport")
	}
	apply := func(previous, next []string) error {
		return link.replaceRelays(context.WithoutCancel(scoped), previous, next)
	}
	if err := apply(previous, next); err != nil {
		return errors.Join(err, apply(next, previous))
	}
	probe, expire := link.timeoutContext(scoped, relayProbeTimeout)
	verified := link.relayReady(probe, next)
	expire()
	if verified == nil && scoped.Err() != nil {
		verified = linkNetwork(scoped.Err())
	}
	if verified != nil {
		return errors.Join(verified, apply(next, previous))
	}
	link.mu.Lock()
	selection = cloneNetworkSelection(selection)
	link.selection, link.relayURLs = &selection, next
	link.mu.Unlock()
	return nil
}

func (link *Link) replaceRelays(ctx context.Context, previous, next []string) error {
	for _, relay := range next {
		if err := link.endpoint.InsertRelay(ctx, relay); err != nil {
			return linkNetwork(err)
		}
	}
	var failures []error
	for _, relay := range previous {
		if !containsRelay(next, relay) {
			failures = append(failures, linkNetwork(link.endpoint.RemoveRelay(ctx, relay)))
		}
	}
	return errors.Join(failures...)
}

func (link *Link) relayReady(ctx context.Context, relays []string) error {
	statuses, err := link.endpoint.RelayStatus(ctx)
	if err != nil {
		return linkNetwork(err)
	}
	address, err := link.endpoint.Address(ctx)
	if err != nil {
		return linkNetwork(err)
	}
	watching, stop := context.WithCancel(ctx)
	defer stop()
	type statusResult struct {
		statuses []iroh.RelayStatus
		err      error
	}
	type addressResult struct {
		address EndpointAddress
		err     error
	}
	statusUpdates := make(chan statusResult)
	addressUpdates := make(chan addressResult)
	statusDone, addressDone := make(chan struct{}), make(chan struct{})
	go func(previous []iroh.RelayStatus) {
		defer close(statusDone)
		for {
			next, err := link.endpoint.WaitRelayStatus(watching, previous)
			select {
			case statusUpdates <- statusResult{next, err}:
			case <-watching.Done():
				return
			}
			if err != nil {
				return
			}
			previous = next
		}
	}(statuses)
	go func(previous EndpointAddress) {
		defer close(addressDone)
		for {
			next, err := link.endpoint.WaitAddress(watching, previous)
			select {
			case addressUpdates <- addressResult{next, err}:
			case <-watching.Done():
				return
			}
			if err != nil {
				return
			}
			previous = next
		}
	}(address)
	defer func() {
		stop()
		<-statusDone
		<-addressDone
	}()
	for {
		ready, denied := selectedRelayReady(statuses, address, relays)
		if ready {
			return nil
		}
		if denied {
			return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "relay authentication denied"}
		}
		select {
		case <-ctx.Done():
			return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "relay connection timed out or cancelled"}
		case update := <-statusUpdates:
			if update.err != nil {
				return linkNetwork(update.err)
			}
			statuses = update.statuses
		case update := <-addressUpdates:
			if update.err != nil {
				return linkNetwork(update.err)
			}
			address = update.address
		}
	}
}

func selectedRelayReady(statuses []iroh.RelayStatus, address EndpointAddress, relays []string) (bool, bool) {
	connected, denied, advertised := false, false, false
	for _, status := range statuses {
		if containsRelay(relays, status.URL) {
			connected = connected || status.Connected
			denied = denied || status.AuthDeniedReason != ""
		}
	}
	for _, hint := range address.Addrs {
		relay, ok := relayHint(hint)
		if !ok {
			continue
		}
		if !containsRelay(relays, relay) {
			return false, denied
		}
		advertised = true
	}
	return connected && advertised, denied
}

func CandidateAddress(address EndpointAddress, selection protocol.NetworkSelection) (EndpointAddress, error) {
	relays, err := NetworkRelayURLs(context.Background(), selection)
	if err != nil {
		return EndpointAddress{}, err
	}
	candidate := cloneAddress(address)
	for _, relay := range relays {
		hint, err := json.Marshal(map[string]string{"Relay": relay})
		if err != nil {
			return EndpointAddress{}, invalidLink("invalid relay URL")
		}
		candidate.Addrs = append(candidate.Addrs, hint)
	}
	return mergeAddress(candidate, EndpointAddress{ID: address.ID}), nil
}

func NetworkRelayURLs(ctx context.Context, selection protocol.NetworkSelection) ([]string, error) {
	switch selection.Kind {
	case "iroh":
		if selection.Data != nil {
			return nil, invalidLink("invalid relay selection")
		}
		relays, err := iroh.DefaultRelayURLs(ctx)
		return relays, linkNetwork(err)
	case "custom":
		var relays []string
		switch payload := selection.Data.(type) {
		case protocol.NetworkSelectionCustomPayload:
			relays = payload.Relays
		case *protocol.NetworkSelectionCustomPayload:
			if payload != nil {
				relays = payload.Relays
			}
		}
		if len(relays) == 0 || len(relays) > 8 {
			return nil, invalidLink("expected one to eight relay URLs")
		}
		unique := map[string]bool{}
		for _, value := range relays {
			relay, err := canonicalRelayURL(value)
			if err != nil {
				return nil, err
			}
			unique[relay] = true
		}
		result := make([]string, 0, len(unique))
		for relay := range unique {
			result = append(result, relay)
		}
		sort.Strings(result)
		return result, nil
	default:
		return nil, invalidLink("invalid relay selection")
	}
}

func canonicalRelayURL(value string) (string, error) {
	relay, err := url.Parse(value)
	if err != nil || relay.Scheme != "https" || relay.Hostname() == "" || urlHasCredentials(relay) ||
		relay.RawQuery != "" || relay.ForceQuery || relay.Fragment != "" || strings.Contains(value, "#") || relay.Opaque != "" {
		return "", invalidLink("relay URLs require HTTPS without credentials")
	}
	if err := normalizeURLHost(relay); err != nil {
		return "", err
	}
	if relay.Path == "" {
		relay.Path = "/"
	}
	return relay.ResolveReference(&url.URL{}).String(), nil
}

func normalizeURLHost(relay *url.URL) error {
	host := strings.ToLower(relay.Hostname())
	if ip := net.ParseIP(host); ip != nil {
		host = ip.String()
	} else {
		canonical, err := idna.Lookup.ToASCII(host)
		if err != nil || strings.Contains(host, ":") {
			return invalidLink("invalid relay URL")
		}
		host = canonical
	}
	port := relay.Port()
	if port != "" {
		number, err := strconv.ParseUint(port, 10, 16)
		if err != nil {
			return invalidLink("invalid relay URL")
		}
		port = strconv.FormatUint(number, 10)
	}
	relay.Host = host
	if strings.Contains(host, ":") {
		relay.Host = "[" + host + "]"
	}
	defaultPort := relay.Scheme == "https" && port == "443" || relay.Scheme == "http" && port == "80"
	if port != "" && !defaultPort {
		relay.Host = net.JoinHostPort(host, port)
	}
	if relay.User != nil && !urlHasCredentials(relay) {
		relay.User = nil
	}
	return nil
}

func urlHasCredentials(value *url.URL) bool {
	if value.User == nil {
		return false
	}
	_, password := value.User.Password()
	return value.User.Username() != "" || password
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

func relayHint(hint json.RawMessage) (string, bool) {
	var transport map[string]json.RawMessage
	if json.Unmarshal(hint, &transport) != nil || len(transport) != 1 {
		return "", false
	}
	var relay string
	if value, ok := transport["Relay"]; ok && json.Unmarshal(value, &relay) == nil {
		canonical, err := canonicalRelayURL(relay)
		return canonical, err == nil
	}
	return "", false
}

func containsRelay(relays []string, relay string) bool {
	for _, candidate := range relays {
		if candidate == relay {
			return true
		}
	}
	return false
}
