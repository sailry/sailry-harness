package mobile

import (
	"context"
	"encoding/json"
	"slices"
	"sync"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type peerRoute struct {
	Entry  link.EndpointAddress
	Target link.EndpointAddress
}

func decodePeerRoute(encoded string) (peerRoute, error) {
	var target link.EndpointAddress
	if err := json.Unmarshal([]byte(encoded), &target); err != nil {
		return peerRoute{}, invalid("invalid peer address")
	}
	var metadata struct {
		Entry *link.EndpointAddress `json:"entry_address"`
	}
	if err := json.Unmarshal([]byte(encoded), &metadata); err != nil {
		return peerRoute{}, invalid("invalid peer route")
	}
	entry := target
	if metadata.Entry != nil {
		entry = *metadata.Entry
	}
	return peerRoute{Entry: entry, Target: target}, nil
}

func (owner *controller) peers(ctx context.Context) ([]string, error) {
	peers, err := owner.endpoint.Peers(ctx)
	if err != nil {
		return nil, err
	}
	relationships, err := owner.endpoint.Relationships(ctx)
	if err != nil {
		return nil, err
	}
	metadata := map[protocol.NodeID]link.PeerRelationship{}
	for _, value := range relationships {
		metadata[value.ID] = value
	}
	entries := []link.EndpointAddress{}
	result := []string{}
	active := map[protocol.NodeID]bool{}
	for _, entry := range peers {
		relationship := metadata[entry.ID]
		if !relationship.Direct || relationship.Removed || !relationship.DirectDirection.AllowsOutbound() {
			continue
		}
		encoded, err := peerAddressMetadata(entry, relationship)
		if err != nil {
			return nil, err
		}
		active[entry.ID] = true
		entries = append(entries, entry)
		result = append(result, encoded)
	}
	var wait sync.WaitGroup
	var results sync.Mutex
	places := make(chan struct{}, 4)
	for _, entry := range entries {
		wait.Add(1)
		go func() {
			defer wait.Done()
			select {
			case places <- struct{}{}:
				defer func() { <-places }()
			case <-ctx.Done():
				return
			}
			children, err := owner.resourceRoutes(ctx, entry)
			if err != nil {
				owner.mu.Lock()
				children = append([]string{}, owner.directories[entry.ID]...)
				owner.mu.Unlock()
			} else {
				owner.mu.Lock()
				if owner.directories == nil {
					owner.directories = map[protocol.NodeID][]string{}
				}
				owner.directories[entry.ID] = append([]string{}, children...)
				owner.mu.Unlock()
			}
			results.Lock()
			result = append(result, children...)
			results.Unlock()
		}()
	}
	wait.Wait()
	owner.mu.Lock()
	removed := []protocol.NodeID{}
	for entry := range owner.directories {
		if !active[entry] {
			removed = append(removed, entry)
		}
	}
	owner.mu.Unlock()
	for _, entry := range removed {
		owner.closeEntry(entry)
	}
	slices.Sort(result)
	return result, ctx.Err()
}

// resourceRoutes reads one controller's view. Directory members remain routes
// through that controller and never become additional trusted Link peers.
func (owner *controller) resourceRoutes(ctx context.Context, entry link.EndpointAddress) ([]string, error) {
	target := client.New(owner.endpoint.Remote(entry))
	defer target.Close()
	catalog, err := target.ReadResourceCatalog(ctx)
	if err != nil {
		return nil, err
	}
	return catalogPeerRoutes(entry, catalog)
}

func catalogPeerRoutes(entry link.EndpointAddress, catalog protocol.ResourceCatalog) ([]string, error) {
	if catalog.Entry != entry.ID {
		return nil, invalid("resource catalog belongs to another entry")
	}
	result := []string{}
	seen := map[protocol.NodeID]bool{entry.ID: true}
	for _, source := range catalog.Sources {
		if source.Node == (protocol.NodeID{}) || seen[source.Node] {
			continue
		}
		seen[source.Node] = true
		encoded, err := routedPeerMetadata(entry, link.EndpointAddress{ID: source.Node, Addrs: []json.RawMessage{}}, link.PeerRelationship{
			ID: source.Node, Name: source.Name, Sources: []protocol.NodeID{entry.ID},
		})
		if err != nil {
			return nil, err
		}
		result = append(result, encoded)
	}
	return result, nil
}

func (owner *controller) closeEntry(entry protocol.NodeID) {
	owner.mu.Lock()
	var clients []*client.Client
	for route, shared := range owner.clients {
		if route.Entry == entry {
			clients = append(clients, shared)
			delete(owner.clients, route)
		}
	}
	delete(owner.directories, entry)
	owner.mu.Unlock()
	for _, shared := range clients {
		_ = shared.Close()
	}
}

func (owner *controller) inbox(route protocol.RouteKey) *client.Inbox {
	owner.mu.Lock()
	defer owner.mu.Unlock()
	if owner.inboxes == nil {
		owner.inboxes = map[protocol.RouteKey]*client.Inbox{}
	}
	if owner.inboxes[route] == nil {
		owner.inboxes[route] = &client.Inbox{}
	}
	return owner.inboxes[route]
}

func (owner *controller) notifications() (string, error) {
	owner.mu.Lock()
	inboxes := make(map[protocol.RouteKey]*client.Inbox, len(owner.inboxes))
	for route, inbox := range owner.inboxes {
		inboxes[route] = inbox
	}
	owner.mu.Unlock()
	routes := make([]protocol.RouteKey, 0, len(inboxes))
	for route := range inboxes {
		routes = append(routes, route)
	}
	slices.SortFunc(routes, func(first, second protocol.RouteKey) int {
		if order := slices.Compare(first.Entry[:], second.Entry[:]); order != 0 {
			return order
		}
		return slices.Compare(first.Target[:], second.Target[:])
	})
	notices := []json.RawMessage{}
	unread := 0
	for _, route := range routes {
		inbox := inboxes[route]
		unread += inbox.Unread()
		for _, notice := range inbox.Notices() {
			body, err := json.Marshal(notice)
			if err != nil {
				return "", err
			}
			var value map[string]json.RawMessage
			if err := json.Unmarshal(body, &value); err != nil {
				return "", err
			}
			var id map[string]json.RawMessage
			if err := json.Unmarshal(value["id"], &id); err != nil {
				return "", err
			}
			id["entry"], err = json.Marshal(route.Entry)
			if err != nil {
				return "", err
			}
			value["id"], err = json.Marshal(id)
			if err != nil {
				return "", err
			}
			routed, err := json.Marshal(value)
			if err != nil {
				return "", err
			}
			notices = append(notices, routed)
		}
	}
	return encode(struct {
		Notices []json.RawMessage `json:"notices"`
		Unread  int               `json:"unread"`
	}{notices, unread})
}
