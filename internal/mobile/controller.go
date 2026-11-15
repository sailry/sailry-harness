package mobile

import (
	"context"
	"encoding/json"
	"os"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type controller struct {
	endpoint       *link.Link
	claim          link.PairingClaim
	inbox          client.Inbox
	mu             sync.Mutex
	clients        map[protocol.NodeID]*client.Client
	identityCancel context.CancelFunc
	identityDone   chan struct{}
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
		owner := &controller{endpoint: endpoint, clients: map[protocol.NodeID]*client.Client{}}
		owner.startIdentitySync(lifetime)
		return runtime.add(0, "controller", owner, lifetime, cancel)
	case "new":
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
		relationships, err := owner.endpoint.Relationships(ctx)
		if err != nil {
			return nil, err
		}
		metadata := map[protocol.NodeID]link.PeerRelationship{}
		for _, relationship := range relationships {
			metadata[relationship.ID] = relationship
		}
		result := make([]string, 0, len(peers))
		for _, peer := range peers {
			encoded, err := peerAddressMetadata(peer, metadata[peer.ID])
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
		// Registered first: closing the connection, also when it cannot be
		// added, removes exactly this Client again.
		owner.mu.Lock()
		owner.clients[address.ID] = shared
		owner.mu.Unlock()
		return runtime.add(handle, "connection", connection, lifetime, cancel)
	case "notifications":
		return encode(struct {
			Notices []client.Notice `json:"notices"`
			Unread  int             `json:"unread"`
		}{Notices: owner.inbox.Notices(), Unread: owner.inbox.Unread()})
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

// Routing remains the ordinary Link address. The optional relationship member
// is local controller metadata for presentation; it never grants peer trust.
func peerAddressMetadata(address link.EndpointAddress, relationship link.PeerRelationship) (string, error) {
	body, err := json.Marshal(address)
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
	return encode(value)
}

func (owner *controller) close() error {
	if owner.identityCancel != nil {
		owner.identityCancel()
		<-owner.identityDone
	}
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

func (owner *controller) startIdentitySync(ctx context.Context) {
	ctx, owner.identityCancel = context.WithCancel(ctx)
	owner.identityDone = make(chan struct{})
	go func() {
		defer close(owner.identityDone)
		ticker := time.NewTicker(5 * time.Second)
		defer ticker.Stop()
		for {
			addresses, err := owner.endpoint.Peers(ctx)
			if err == nil {
				for _, address := range addresses {
					if ctx.Err() != nil {
						return
					}
					if !owner.endpoint.Direction(address.ID).AllowsOutbound() {
						continue
					}
					remote := client.New(owner.endpoint.Remote(address))
					_ = client.SyncIdentity(ctx, owner.endpoint, remote)
					_ = remote.Close()
				}
			}
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
			}
		}
	}()
}
