package mobile

import (
	"context"
	"encoding/json"
	"maps"
	"os"
	"slices"
	"sync"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type controller struct {
	endpoint    *link.Link
	claim       link.PairingClaim
	mu          sync.Mutex
	clients     map[protocol.RouteKey]*client.Client
	inboxes     map[protocol.RouteKey]*client.Inbox
	directories map[protocol.NodeID][]string
	topology    uint64
	changing    uint64
}

type input struct {
	Kind       string   `json:"kind"`
	Path       string   `json:"path"`
	Internet   bool     `json:"internet"`
	Relays     []string `json:"relays"`
	Name       string   `json:"name"`
	Ticket     string   `json:"ticket"`
	Origin     string   `json:"origin"`
	PIN        string   `json:"pin"`
	Address    string   `json:"address"`
	Addresses  []string `json:"addresses"`
	Query      string   `json:"query"`
	Command    string   `json:"command"`
	Request    string   `json:"request"`
	Scope      string   `json:"scope"`
	Entry      string   `json:"entry"`
	Session    string   `json:"session"`
	Terminal   string   `json:"terminal"`
	Source     string   `json:"source"`
	LocalPort  uint16   `json:"local_port"`
	RemotePort uint16   `json:"remote_port"`
	Upload     string   `json:"upload"`
	Download   string   `json:"download"`
	Iroh       string   `json:"iroh"`
}

func (runtime *Runtime) create(ctx context.Context, method string, data json.RawMessage) (any, error) {
	var values input
	if err := arguments(data, &values); err != nil {
		return nil, err
	}
	switch method {
	case "initialize":
		path := os.Getenv("SAILRY_IROH_LIB")
		if path == "" {
			path = values.Iroh
		}
		if path == "" {
			return nil, invalid("iroh library path is required")
		}
		return nil, iroh.Load(path)
	case "open":
		scope := link.NetworkScope{Internet: values.Internet, Relays: values.Relays}
		if len(values.Relays) != 0 {
			scope.Internet = false
		}
		lifetime, cancel, detach := runtime.lifetime(ctx, 0)
		endpoint, err := link.OpenController(lifetime, values.Path, scope)
		detach()
		if err != nil {
			cancel()
			return nil, err
		}
		owner := &controller{endpoint: endpoint, clients: map[protocol.RouteKey]*client.Client{}}
		return runtime.add(0, "controller", owner, lifetime, cancel)
	case "new":
		lifetime, cancel, detach := runtime.lifetime(ctx, 0)
		detach()
		switch values.Kind {
		case "digest":
			return runtime.add(0, "digest", newDigest(), lifetime, cancel)
		default:
			cancel()
		}
	}
	return nil, invalid("unknown native root operation")
}

func (owner *controller) invoke(ctx context.Context, runtime *Runtime, handle uint64, method string, data json.RawMessage) (any, error) {
	var values input
	if err := arguments(data, &values); err != nil {
		return nil, err
	}
	switch method {
	case "id":
		address, err := owner.endpoint.Address(ctx)
		if err != nil {
			return nil, err
		}
		return encode(address.ID)
	case "set_name":
		return nil, owner.endpoint.SetName(values.Name)
	case "pair":
		owner.beginTopologyChange()
		defer owner.endTopologyChange()
		address, err := owner.endpoint.Pair(ctx, values.Ticket)
		if err != nil {
			return nil, err
		}
		return encode(address)
	case "pair_pin":
		owner.beginTopologyChange()
		defer owner.endTopologyChange()
		address, err := owner.endpoint.PairPIN(ctx, values.Origin, values.PIN, &owner.claim)
		if err != nil {
			return nil, err
		}
		return encode(address)
	case "peers":
		return owner.peers(ctx)
	case "saved_peers":
		return owner.savedPeers(ctx)
	case "forget":
		route, err := decodePeerRoute(values.Address)
		if err != nil {
			return nil, err
		}
		if route.Target.ID != route.Entry.ID {
			return nil, invalid("remove the entry Node to close its resource routes")
		}
		owner.beginTopologyChange()
		defer owner.endTopologyChange()
		_, err = client.RemoveHost(ctx, owner.endpoint, route.Entry, func(local context.Context) error { return owner.endpoint.SetTrust(local, route.Entry.ID, false) })
		if err == nil {
			owner.closeEntry(route.Entry.ID)
		}
		return nil, err
	case "network_changed":
		err := owner.endpoint.NetworkChanged(ctx)
		owner.mu.Lock()
		clients := slices.Collect(maps.Values(owner.clients))
		owner.mu.Unlock()
		for _, client := range clients {
			client.Retry()
		}
		return nil, err
	case "connect":
		route, err := decodePeerRoute(values.Address)
		if err != nil {
			return nil, err
		}
		lifetime, cancel, detach := runtime.lifetime(ctx, handle)
		detach()
		shared := client.New(owner.endpoint.Via(route.Entry, route.Target.ID))
		connection := &connection{client: shared, controller: owner}
		// Registered first: closing the connection, also when it cannot be
		// added, removes exactly this Client again.
		owner.mu.Lock()
		if owner.clients == nil {
			owner.clients = map[protocol.RouteKey]*client.Client{}
		}
		owner.clients[shared.Route()] = shared
		owner.topology++
		owner.mu.Unlock()
		return runtime.add(handle, "connection", connection, lifetime, cancel)
	case "notifications":
		return owner.notifications()
	case "watch_usage":
		if len(values.Addresses) == 0 {
			return nil, invalid("usage overview requires an execution Node")
		}
		lifetime, cancel, detach := runtime.lifetime(ctx, handle)
		clients := make([]*client.Client, 0, len(values.Addresses))
		for _, encoded := range values.Addresses {
			route, err := decodePeerRoute(encoded)
			if err != nil {
				detach()
				cancel()
				return nil, err
			}
			clients = append(clients, client.New(owner.endpoint.Via(route.Entry, route.Target.ID)))
		}
		observer, err := client.WatchOverview(lifetime, clients, json.RawMessage(values.Query))
		detach()
		if err != nil {
			cancel()
			for _, shared := range clients {
				_ = shared.Close()
			}
			return nil, err
		}
		return runtime.add(handle, "overview_updates", &observation{observer: observer, clients: clients}, lifetime, cancel)
	}
	return nil, invalid("unknown controller operation")
}

// Metadata preserves the actual resource identity and the authenticated entry.
func peerAddressMetadata(address link.EndpointAddress, relationship link.PeerRelationship) (string, error) {
	return routedPeerMetadata(address, address, relationship, nil)
}

func routedPeerMetadata(entry, target link.EndpointAddress, relationship link.PeerRelationship, fault *protocol.Fault) (string, error) {
	body, err := json.Marshal(target)
	if err != nil {
		return "", err
	}
	var value map[string]any
	if err := json.Unmarshal(body, &value); err != nil {
		return "", err
	}
	sources := make([]string, 0, len(relationship.Sources))
	for _, source := range relationship.Sources {
		sources = append(sources, source.String())
	}
	value["relationship"] = map[string]any{"direct": relationship.Direct, "sources": sources, "name": relationship.Name}
	if fault != nil {
		value["source_fault"] = fault
	}
	value["entry_address"] = entry
	value["route"] = map[string]string{"entry": entry.ID.String(), "target": target.ID.String()}
	return encode(value)
}

func (owner *controller) close() error {
	owner.mu.Lock()
	owner.topology++
	clients := make([]*client.Client, 0, len(owner.clients))
	for _, shared := range owner.clients {
		clients = append(clients, shared)
	}
	owner.clients = map[protocol.RouteKey]*client.Client{}
	owner.mu.Unlock()
	for _, shared := range clients {
		_ = shared.Close()
	}
	return owner.endpoint.Close()
}
