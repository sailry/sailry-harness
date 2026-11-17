package mobile

import (
	"context"
	"encoding/json"
	"errors"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type routedNoticeTransport struct {
	noticeTransport
	entry protocol.NodeID
}

var errCatalogLifecycleProbe = errors.New("catalog lifecycle probe reached the transport")

type catalogLifecycleTransport struct{ noticeTransport }

func (catalogLifecycleTransport) Subscribe(context.Context, protocol.Topic) (link.Subscription, error) {
	return nil, errCatalogLifecycleProbe
}

func TestMobileStaleCatalogCannotChangeRepairedEntry(t *testing.T) {
	for _, outcome := range []string{"success", "denied", "missing"} {
		t.Run(outcome, func(t *testing.T) {
			entry := protocol.NodeID{1}
			shared := client.New(catalogLifecycleTransport{noticeTransport{node: entry}})
			t.Cleanup(func() { _ = shared.Close() })
			owner := &controller{clients: map[protocol.RouteKey]*client.Client{shared.Route(): shared}, directories: map[protocol.NodeID][]string{entry: {"new catalog"}}}
			inbox := owner.inbox(shared.Route())
			captured := owner.topology
			owner.beginTopologyChange()
			owner.endTopologyChange()
			catalogs := []peerCatalog{{entry: entry, direct: "old direct", children: []string{"old catalog"}}}
			if outcome == "denied" {
				catalogs[0].failure = &protocol.Fault{Code: protocol.ErrorCodePermissionDenied}
			} else if outcome == "missing" {
				catalogs = nil
			}
			_, err := owner.commitPeerCatalogs(captured, catalogs)
			var failure *protocol.Fault
			if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeConflict {
				t.Fatalf("stale catalog was accepted: %v", err)
			}
			if owner.clients[shared.Route()] != shared || owner.inboxes[shared.Route()] != inbox || owner.directories[entry][0] != "new catalog" {
				t.Fatal("stale result modified the new entry")
			}
			if err := shared.Start(t.Context()); !errors.Is(err, errCatalogLifecycleProbe) {
				t.Fatalf("stale result closed the new Client: %v", err)
			}
		})
	}
}

func TestMobileCatalogRejectsOverlappingTopologyChange(t *testing.T) {
	owner := &controller{}
	owner.beginTopologyChange()
	owner.beginTopologyChange()
	owner.endTopologyChange()
	if _, err := owner.commitPeerCatalogs(owner.topology, nil); err == nil {
		t.Fatal("catalog committed while a relationship operation was still active")
	}
	owner.endTopologyChange()
	if _, err := owner.commitPeerCatalogs(owner.topology, nil); err != nil {
		t.Fatalf("settled topology rejected its current catalog: %v", err)
	}
}

func TestMobileDetachedEntryCannotCloseReplacement(t *testing.T) {
	entry := protocol.NodeID{1}
	transport := catalogLifecycleTransport{noticeTransport{node: entry}}
	previous, replacement := client.New(transport), client.New(transport)
	t.Cleanup(func() { _ = previous.Close(); _ = replacement.Close() })
	owner := &controller{clients: map[protocol.RouteKey]*client.Client{previous.Route(): previous}}
	owner.mu.Lock()
	detached := owner.detachMissingEntries(map[protocol.NodeID]bool{})
	owner.clients[replacement.Route()] = replacement
	owner.mu.Unlock()
	for _, shared := range detached {
		_ = shared.Close()
	}
	if owner.clients[replacement.Route()] != replacement {
		t.Fatal("closing old handles removed the replacement")
	}
	if err := replacement.Start(t.Context()); !errors.Is(err, errCatalogLifecycleProbe) {
		t.Fatalf("closing old handles closed the replacement: %v", err)
	}
	if err := previous.Start(t.Context()); err == nil || errors.Is(err, errCatalogLifecycleProbe) {
		t.Fatal("detached Client was not closed")
	}
}

func (transport routedNoticeTransport) Route() protocol.RouteKey {
	return protocol.RouteKey{Entry: transport.entry, Target: transport.node}
}

func TestMobileRouteMetadataKeepsEntryAndTargetSeparate(t *testing.T) {
	entry := link.EndpointAddress{ID: protocol.NodeID{1}, Addrs: []json.RawMessage{json.RawMessage(`{"Ip":"127.0.0.1:4321"}`)}}
	target := link.EndpointAddress{ID: protocol.NodeID{2}, Addrs: []json.RawMessage{}}
	encoded, err := routedPeerMetadata(entry, target, link.PeerRelationship{ID: target.ID, Sources: []protocol.NodeID{entry.ID}}, nil)
	if err != nil {
		t.Fatal(err)
	}
	route, err := decodePeerRoute(encoded)
	if err != nil || route.Entry.ID != entry.ID || route.Target.ID != target.ID || len(route.Entry.Addrs) != 1 || len(route.Target.Addrs) != 0 {
		t.Fatalf("route metadata changed endpoint ownership: %#v %v", route, err)
	}
	direct, err := peerAddressMetadata(target, link.PeerRelationship{ID: target.ID, Direct: true})
	if err != nil {
		t.Fatal(err)
	}
	directRoute, err := decodePeerRoute(direct)
	if err != nil || directRoute.Entry.ID != target.ID || directRoute.Target.ID != target.ID {
		t.Fatalf("direct route was changed: %#v %v", directRoute, err)
	}
}

func TestMobileNotificationsRetainSeparateEntryNamespaces(t *testing.T) {
	owner := &controller{}
	target, entry := protocol.NodeID{2}, protocol.NodeID{1}
	direct := protocol.RouteKey{Entry: target, Target: target}
	via := protocol.RouteKey{Entry: entry, Target: target}
	notice := client.Notice{
		ID:   client.NoticeID{Node: target, Cursor: 8, Target: client.NoticeTarget{Kind: "notification", Notification: protocol.NewNotificationID()}},
		Kind: "notification", NotificationKind: protocol.NotificationKindInfo,
	}
	owner.inbox(direct).Merge(target, []client.Notice{notice})
	owner.inbox(via).Merge(target, []client.Notice{notice})
	value, err := owner.notifications()
	if err != nil {
		t.Fatal(err)
	}
	var result struct {
		Notices []struct {
			ID struct {
				Node  protocol.NodeID `json:"node"`
				Entry protocol.NodeID `json:"entry"`
			} `json:"id"`
		} `json:"notices"`
		Unread int `json:"unread"`
	}
	if err := json.Unmarshal([]byte(value), &result); err != nil {
		t.Fatal(err)
	}
	if len(result.Notices) != 2 || result.Unread != 2 || result.Notices[0].ID.Node != target ||
		result.Notices[1].ID.Node != target || result.Notices[0].ID.Entry == result.Notices[1].ID.Entry {
		t.Fatalf("entry-specific notifications were merged: %+v", result)
	}
}

func TestMobileClosingEntryPreservesIndependentRoute(t *testing.T) {
	target, entry := protocol.NodeID{2}, protocol.NodeID{1}
	direct := client.New(noticeTransport{node: target})
	via := client.New(routedNoticeTransport{noticeTransport: noticeTransport{node: target}, entry: entry})
	t.Cleanup(func() { _ = direct.Close(); _ = via.Close() })
	owner := &controller{clients: map[protocol.RouteKey]*client.Client{direct.Route(): direct, via.Route(): via}}
	owner.inbox(direct.Route())
	owner.inbox(via.Route())
	owner.closeEntry(entry)
	if owner.clients[direct.Route()] != direct || owner.clients[via.Route()] != nil {
		t.Fatal("closing one entry changed an independent route to the same Node")
	}
	if owner.inboxes[direct.Route()] == nil || owner.inboxes[via.Route()] != nil {
		t.Fatal("closing one entry retained its notification inbox or removed another entry")
	}
}

func TestMobileRemovedEntryClosesBeforeItsFirstCatalog(t *testing.T) {
	entry, target := protocol.NodeID{1}, protocol.NodeID{2}
	shared := client.New(routedNoticeTransport{noticeTransport: noticeTransport{node: target}, entry: entry})
	t.Cleanup(func() { _ = shared.Close() })
	owner := &controller{clients: map[protocol.RouteKey]*client.Client{shared.Route(): shared}}
	owner.inbox(shared.Route())
	if _, err := owner.commitPeerCatalogs(owner.topology, nil); err != nil {
		t.Fatal(err)
	}
	if len(owner.clients) != 0 || len(owner.inboxes) != 0 {
		t.Fatal("entry with no successful directory read retained its client or inbox")
	}
	if err := shared.Start(t.Context()); err == nil {
		t.Fatal("removed entry's Client was not closed")
	}
}

func TestMobileSourceFaultSurvivesCatalogAndEntryOutage(t *testing.T) {
	entry, target := protocol.NodeID{1}, protocol.NodeID{2}
	address := link.EndpointAddress{ID: entry, Addrs: []json.RawMessage{}}
	catalog := protocol.ResourceCatalog{Entry: entry, Sources: []protocol.ResourceSource{{Node: target, Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "offline"}}}}
	routes, err := catalogPeerRoutes(address, catalog)
	if err != nil || len(routes) != 1 {
		t.Fatalf("source route: %v: %v", routes, err)
	}
	for _, encoded := range []string{routes[0], unavailablePeerRoutes(routes)[0]} {
		var metadata struct {
			Fault *protocol.Fault `json:"source_fault"`
		}
		if err := json.Unmarshal([]byte(encoded), &metadata); err != nil || metadata.Fault == nil || metadata.Fault.Code != protocol.ErrorCodeUnavailable {
			t.Fatalf("source outage was discarded: %s: %v", encoded, err)
		}
		route, err := decodePeerRoute(encoded)
		if err != nil || route.Entry.ID != entry || route.Target.ID != target {
			t.Fatalf("offline metadata changed the route: %+v: %v", route, err)
		}
	}
}

type mobileCatalogTransport struct {
	noticeTransport
	catalog protocol.ResourceCatalog
}

func (transport mobileCatalogTransport) Dispatch(_ context.Context, request protocol.Request) (link.Admission, error) {
	completed := make(chan link.Response, 1)
	completed <- link.Response{Output: protocol.Output{Kind: "resource_catalog", Data: transport.catalog}}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completed}, nil
}

func TestMobileCatalogCreatesRoutesWithoutPairingMembers(t *testing.T) {
	entry, target := protocol.NodeID{1}, protocol.NodeID{2}
	catalog := protocol.ResourceCatalog{Entry: entry, Sources: []protocol.ResourceSource{
		{Node: entry, Route: protocol.RouteKey{Entry: entry, Target: entry}, Name: "Desktop"},
		{Node: target, Route: protocol.RouteKey{Entry: entry, Target: target}, Name: "Build host"},
	}}
	shared := client.New(mobileCatalogTransport{noticeTransport: noticeTransport{node: entry}, catalog: catalog})
	t.Cleanup(func() { _ = shared.Close() })
	value, err := shared.ReadResourceCatalog(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	address := link.EndpointAddress{ID: entry, Addrs: []json.RawMessage{}}
	encoded, err := catalogPeerRoutes(address, value)
	if err != nil || len(encoded) != 1 {
		t.Fatalf("catalog members were not routed: %v %v", encoded, err)
	}
	route, err := decodePeerRoute(encoded[0])
	if err != nil || route.Entry.ID != entry || route.Target.ID != target {
		t.Fatalf("catalog route lost its authority: %#v %v", route, err)
	}
}

func TestMobileRouteIncludesDurableDirectorySummary(t *testing.T) {
	entry, target := protocol.NodeID{1}, protocol.NodeID{2}
	project := protocol.ProjectID{3}
	synced := uint64(1234)
	catalog := protocol.ResourceCatalog{Entry: entry, Sources: []protocol.ResourceSource{{
		Node: target, Route: protocol.RouteKey{Entry: entry, Target: target}, Revision: 8, LastSyncedAtMs: &synced,
		Projects: []protocol.Project{{ID: project, Name: "Cached project", Path: "/remote/project"}},
		Fault:    &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "offline"},
	}}}
	routes, err := catalogPeerRoutes(link.EndpointAddress{ID: entry}, catalog)
	if err != nil || len(routes) != 1 {
		t.Fatalf("cached routes = %v: %v", routes, err)
	}
	var metadata struct {
		Source protocol.ResourceSource `json:"resource_source"`
	}
	if err := json.Unmarshal([]byte(routes[0]), &metadata); err != nil {
		t.Fatal(err)
	}
	if metadata.Source.Node != target || metadata.Source.Route.Entry != entry || len(metadata.Source.Projects) != 1 ||
		metadata.Source.Projects[0].ID != project || metadata.Source.LastSyncedAtMs == nil || *metadata.Source.LastSyncedAtMs != synced {
		t.Fatalf("mobile cold directory lost identity or summary: %#v", metadata.Source)
	}
}
