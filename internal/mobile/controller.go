package mobile

import (
	"context"
	"encoding/json"
	"os"
	"sync"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type controller struct {
	endpoint *link.Link
	claim    link.PairingClaim
	inbox    client.Inbox
	mu       sync.Mutex
	clients  map[protocol.NodeID]*client.Client
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
	ID         string   `json:"id"`
	Query      string   `json:"query"`
	Command    string   `json:"command"`
	Request    string   `json:"request"`
	Scope      string   `json:"scope"`
	Entry      string   `json:"entry"`
	Session    string   `json:"session"`
	Terminal   string   `json:"terminal"`
	Attempt    string   `json:"attempt"`
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
		owner := &controller{endpoint: endpoint, clients: map[protocol.NodeID]*client.Client{}}
		return runtime.add(0, "controller", owner, lifetime, cancel)
	case "new", "default":
		lifetime, cancel, detach := runtime.lifetime(ctx, 0)
		detach()
		switch values.Kind {
		case "digest":
			return runtime.add(0, "digest", newDigest(), lifetime, cancel)
		case "speech_input":
			return runtime.add(0, "speech_input", &speech{}, lifetime, cancel)
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
		address, err := owner.endpoint.Pair(ctx, values.Ticket)
		if err != nil {
			return nil, err
		}
		return encode(address)
	case "pair_pin":
		address, err := owner.endpoint.PairPIN(ctx, values.Origin, values.PIN, &owner.claim)
		if err != nil {
			return nil, err
		}
		return encode(address)
	case "peers":
		peers, err := owner.endpoint.Peers(ctx)
		if err != nil {
			return nil, err
		}
		result := make([]string, 0, len(peers))
		for _, peer := range peers {
			encoded, err := encode(peer)
			if err != nil {
				return nil, err
			}
			result = append(result, encoded)
		}
		return result, nil
	case "forget":
		var address link.EndpointAddress
		if err := json.Unmarshal([]byte(values.Address), &address); err != nil {
			return nil, invalid("invalid peer address")
		}
		return nil, owner.endpoint.SetTrust(ctx, address.ID, false)
	case "network_changed":
		return nil, owner.endpoint.NetworkChanged(ctx)
	case "connect":
		var address link.EndpointAddress
		if err := json.Unmarshal([]byte(values.Address), &address); err != nil {
			return nil, invalid("invalid peer address")
		}
		lifetime, cancel, detach := runtime.lifetime(ctx, handle)
		detach()
		shared := client.New(owner.endpoint.Remote(address))
		connection := &connection{client: shared, controller: owner}
		result, err := runtime.add(handle, "connection", connection, lifetime, cancel)
		if err == nil {
			owner.mu.Lock()
			owner.clients[address.ID] = shared
			owner.mu.Unlock()
		}
		return result, err
	case "notifications":
		return encode(struct {
			Notices []client.Notice `json:"notices"`
			Unread  int             `json:"unread"`
		}{Notices: owner.inbox.Notices(), Unread: owner.inbox.Unread()})
	case "mark_notification_read", "mark_notifications_read":
		clients, err := owner.notificationClients(ctx)
		if err != nil {
			return nil, err
		}
		if method == "mark_notifications_read" {
			return nil, owner.inbox.MarkAllRead(ctx, clients)
		}
		var id client.NoticeID
		if err := json.Unmarshal([]byte(values.ID), &id); err != nil {
			return nil, invalid("invalid notification identifier")
		}
		return nil, owner.inbox.MarkRead(ctx, id, clients)
	case "watch_usage":
		if len(values.Addresses) == 0 {
			return nil, invalid("usage overview requires an execution Node")
		}
		lifetime, cancel, detach := runtime.lifetime(ctx, handle)
		clients := make([]*client.Client, 0, len(values.Addresses))
		for _, encoded := range values.Addresses {
			var address link.EndpointAddress
			if err := json.Unmarshal([]byte(encoded), &address); err != nil {
				detach()
				cancel()
				return nil, invalid("invalid peer address")
			}
			clients = append(clients, client.New(owner.endpoint.Remote(address)))
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

func (owner *controller) notificationClients(ctx context.Context) (map[protocol.NodeID]*client.Client, error) {
	peers, err := owner.endpoint.Peers(ctx)
	if err != nil {
		return nil, err
	}
	owner.mu.Lock()
	defer owner.mu.Unlock()
	result := map[protocol.NodeID]*client.Client{}
	for _, address := range peers {
		shared := owner.clients[address.ID]
		if shared == nil {
			shared = client.New(owner.endpoint.Remote(address))
			owner.clients[address.ID] = shared
		}
		result[address.ID] = shared
	}
	return result, nil
}

func (owner *controller) close() error {
	owner.mu.Lock()
	clients := make([]*client.Client, 0, len(owner.clients))
	for _, shared := range owner.clients {
		clients = append(clients, shared)
	}
	owner.clients = map[protocol.NodeID]*client.Client{}
	owner.mu.Unlock()
	for _, shared := range clients {
		_ = shared.Close()
	}
	return owner.endpoint.Close()
}
