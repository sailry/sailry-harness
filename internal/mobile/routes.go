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

func (owner *controller) beginTopologyChange() {
	owner.mu.Lock()
	owner.topology++
	owner.changing++
	owner.mu.Unlock()
}

func (owner *controller) endTopologyChange() {
	owner.mu.Lock()
	owner.topology++
	owner.changing--
	owner.mu.Unlock()
}

type peerCatalog struct {
	entry    protocol.NodeID
	address  link.EndpointAddress
	direct   string
	children []string
	failure  *protocol.Fault
}

// savedPeers reads only local pairing metadata. Startup can register these
// Clients before any remote resource directory responds.
func (owner *controller) savedPeers(ctx context.Context) ([]string, error) {
	catalogs, err := owner.savedPeerCatalogs(ctx)
	if err != nil {
		return nil, err
	}
	routes := make([]string, 0, len(catalogs))
	for _, catalog := range catalogs {
		routes = append(routes, catalog.direct)
	}
	slices.Sort(routes)
	return routes, nil
}

func (owner *controller) savedPeerCatalogs(ctx context.Context) ([]peerCatalog, error) {
	peers, err := owner.endpoint.Peers(ctx)
	if err != nil {
		return nil, err
	}
	relationships, err := owner.endpoint.Relationships(ctx)
	if err != nil {
		return nil, err
	}
	return directPeerCatalogs(peers, relationships)
}

func directPeerCatalogs(peers []link.EndpointAddress, relationships []link.PeerRelationship) ([]peerCatalog, error) {
	metadata := map[protocol.NodeID]link.PeerRelationship{}
	for _, value := range relationships {
		metadata[value.ID] = value
	}
	catalogs := []peerCatalog{}
	for _, entry := range peers {
		relationship := metadata[entry.ID]
		if !relationship.Direct || relationship.Removed || !relationship.DirectDirection.AllowsOutbound() {
			continue
		}
		encoded, err := peerAddressMetadata(entry, relationship)
		if err != nil {
			return nil, err
		}
		catalogs = append(catalogs, peerCatalog{entry: entry.ID, address: entry, direct: encoded})
	}
	return catalogs, nil
}

func (owner *controller) peers(ctx context.Context) ([]string, error) {
	owner.mu.Lock()
	generation, changing := owner.topology, owner.changing
	owner.mu.Unlock()
	if changing != 0 {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "controller relationships are changing"}
	}
	catalogs, err := owner.savedPeerCatalogs(ctx)
	if err != nil {
		return nil, err
	}
	var wait sync.WaitGroup
	places := make(chan struct{}, 4)
	for index, catalog := range catalogs {
		wait.Add(1)
		go func() {
			defer wait.Done()
			select {
			case places <- struct{}{}:
				defer func() { <-places }()
			case <-ctx.Done():
				return
			}
			children, err := owner.resourceRoutes(ctx, catalog.address)
			catalogs[index].children = children
			if err != nil {
				catalogs[index].failure = result(nil, err).Error
			}
		}()
	}
	wait.Wait()
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	for i := range catalogs {
		catalog := &catalogs[i]
		if catalog.failure == nil || terminalPeerFault(catalog.failure.Code) {
			continue
		}
		// A cold controller has no previous routes to report during an outage.
		// The app can retain their cached views until this entry lists its routes.
		var direct map[string]json.RawMessage
		if err := json.Unmarshal([]byte(catalog.direct), &direct); err != nil {
			return nil, err
		}
		direct["resource_routes_incomplete"] = json.RawMessage(`true`)
		if catalog.direct, err = encode(direct); err != nil {
			return nil, err
		}
	}
	return owner.commitPeerCatalogs(generation, catalogs)
}

// Network reads cannot mutate the handle graph until their captured topology
// still matches. Detach old Clients while holding the same lock as that check;
// closing them later must not look up a newly paired replacement by route.
func (owner *controller) commitPeerCatalogs(generation uint64, catalogs []peerCatalog) ([]string, error) {
	owner.mu.Lock()
	if owner.topology != generation || owner.changing != 0 {
		owner.mu.Unlock()
		return nil, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "controller relationships changed during directory refresh"}
	}
	if owner.directories == nil {
		owner.directories = map[protocol.NodeID][]string{}
	}
	routes := []string{}
	active := map[protocol.NodeID]bool{}
	for _, catalog := range catalogs {
		children := catalog.children
		if failure := catalog.failure; failure != nil {
			if terminalPeerFault(failure.Code) {
				continue
			}
			children = unavailablePeerRoutes(owner.directories[catalog.entry])
		} else {
			owner.directories[catalog.entry] = append([]string{}, children...)
		}
		active[catalog.entry] = true
		routes = append(routes, catalog.direct)
		routes = append(routes, children...)
	}
	clients := owner.detachMissingEntries(active)
	owner.topology++
	owner.mu.Unlock()
	for _, shared := range clients {
		_ = shared.Close()
	}
	slices.Sort(routes)
	return routes, nil
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
		}, source.Fault)
		if err != nil {
			return nil, err
		}
		var metadata map[string]any
		if err := json.Unmarshal([]byte(encoded), &metadata); err != nil {
			return nil, err
		}
		metadata["resource_source"] = source
		encoded, err = encode(metadata)
		if err != nil {
			return nil, err
		}
		result = append(result, encoded)
	}
	return result, nil
}

func terminalPeerFault(code protocol.ErrorCode) bool {
	return code == protocol.ErrorCodePermissionDenied || code == protocol.ErrorCodeNotFound || code == protocol.ErrorCodeExpired
}

func unavailablePeerRoutes(routes []string) []string {
	result := make([]string, 0, len(routes))
	for _, route := range routes {
		var metadata map[string]any
		if json.Unmarshal([]byte(route), &metadata) != nil {
			continue
		}
		fault, _ := metadata["source_fault"].(map[string]any)
		code, _ := fault["code"].(string)
		if terminalPeerFault(protocol.ErrorCode(code)) {
			result = append(result, route)
			continue
		}
		metadata["source_fault"] = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "resource entry Node is unavailable"}
		if encoded, err := encode(metadata); err == nil {
			result = append(result, encoded)
		}
	}
	return result
}

// detachMissingEntries and detachEntry require owner.mu.
func (owner *controller) detachMissingEntries(active map[protocol.NodeID]bool) []*client.Client {
	removed := []protocol.NodeID{}
	for entry := range owner.directories {
		if !active[entry] {
			removed = append(removed, entry)
		}
	}
	for route := range owner.clients {
		if !active[route.Entry] && !slices.Contains(removed, route.Entry) {
			removed = append(removed, route.Entry)
		}
	}
	for route := range owner.inboxes {
		if !active[route.Entry] && !slices.Contains(removed, route.Entry) {
			removed = append(removed, route.Entry)
		}
	}
	var clients []*client.Client
	for _, entry := range removed {
		clients = append(clients, owner.detachEntry(entry)...)
	}
	return clients
}

func (owner *controller) closeEntry(entry protocol.NodeID) {
	owner.mu.Lock()
	clients := owner.detachEntry(entry)
	owner.topology++
	owner.mu.Unlock()
	for _, shared := range clients {
		_ = shared.Close()
	}
}

func (owner *controller) detachEntry(entry protocol.NodeID) []*client.Client {
	var clients []*client.Client
	for route, shared := range owner.clients {
		if route.Entry == entry {
			clients = append(clients, shared)
			delete(owner.clients, route)
		}
	}
	for route := range owner.inboxes {
		if route.Entry == entry {
			delete(owner.inboxes, route)
		}
	}
	delete(owner.directories, entry)
	return clients
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
