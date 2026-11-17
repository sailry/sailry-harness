package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type resourceCatalogTransport struct {
	target protocol.NodeID
	calls  []protocol.ResourceCall
	read   func(protocol.ResourceCall) (protocol.Output, error)
}

func TestKnowledgePrefixFollowsSourcePages(t *testing.T) {
	query := protocol.DefaultKnowledgeQuery()
	query.Query, query.Limit, query.Offset = "reference", 10, 195
	leaf := query
	leaf.Offset, leaf.Limit = 0, 100
	plugin := &protocol.PluginContext{Package: protocol.PluginReference{Name: "fixture"}}
	request := protocol.NewRequest(protocol.NodeID{4}, protocol.Command{Kind: "query_knowledge", Data: &leaf})
	request.Plugin = plugin
	var offsets []uint32
	output, err := resourceKnowledgePrefix(t.Context(), protocol.ResourceCall{Local: true, Request: request}, &query, func(call protocol.ResourceCall) (protocol.Output, error) {
		input := call.Request.Command.Data.(*protocol.KnowledgeQuery)
		offsets = append(offsets, input.Offset)
		if call.Request.Plugin != plugin || call.Request.Target != request.Target || !call.Local {
			t.Fatal("source paging lost the captured caller or target")
		}
		page := protocol.DefaultKnowledgePage()
		end := min(input.Offset+uint32(input.Limit), 230)
		for index := input.Offset; index < end; index++ {
			page.Hits = append(page.Hits, protocol.KnowledgeHit{Title: fmt.Sprint(index)})
		}
		if end < 230 {
			page.NextOffset = &end
		}
		return protocol.Output{Kind: "knowledge_search", Data: page}, nil
	})
	page, valid := dispatchPayload[protocol.KnowledgePage](output.Data)
	if err != nil || !valid || !reflect.DeepEqual(offsets, []uint32{0, 100, 200}) || len(page.Hits) != 230 || page.NextOffset != nil {
		t.Fatalf("source paging was truncated: offsets=%v page=%+v err=%v", offsets, page, err)
	}
	if page.Hits[195].Title != "195" || page.Hits[204].Title != "204" {
		t.Fatal("source pages were reordered")
	}
}

func TestKnowledgePrefixRetainsUnconsumedSourceCursor(t *testing.T) {
	query := protocol.DefaultKnowledgeQuery()
	query.Query, query.Limit = "reference", 5
	request := protocol.NewRequest(protocol.NodeID{4}, protocol.Command{Kind: "query_knowledge", Data: &query})
	next := uint32(100)
	output, err := resourceKnowledgePrefix(t.Context(), protocol.ResourceCall{Request: request}, &query, func(protocol.ResourceCall) (protocol.Output, error) {
		return protocol.Output{Kind: "knowledge_search", Data: protocol.KnowledgePage{Hits: make([]protocol.KnowledgeHit, 100), NextOffset: &next}}, nil
	})
	page, valid := dispatchPayload[protocol.KnowledgePage](output.Data)
	if err != nil || !valid || page.NextOffset == nil || *page.NextOffset != next {
		t.Fatalf("unconsumed source cursor was discarded: %+v: %v", page, err)
	}
}

func TestKnowledgePrefixRejectsInvalidCursorBeforeReturning(t *testing.T) {
	for _, test := range []struct {
		name string
		hits int
		next uint32
	}{
		{"enough hits with a stationary cursor", 100, 0},
		{"empty page with a next cursor", 0, 1},
	} {
		t.Run(test.name, func(t *testing.T) {
			query := protocol.DefaultKnowledgeQuery()
			query.Query, query.Limit = "reference", 5
			request := protocol.NewRequest(protocol.NodeID{4}, protocol.Command{Kind: "query_knowledge", Data: &query})
			calls := 0
			output, err := resourceKnowledgePrefix(t.Context(), protocol.ResourceCall{Request: request}, &query, func(protocol.ResourceCall) (protocol.Output, error) {
				calls++
				return protocol.Output{Kind: "knowledge_search", Data: protocol.KnowledgePage{Hits: make([]protocol.KnowledgeHit, test.hits), NextOffset: &test.next}}, nil
			})
			if err == nil || resourceFault(err).Code != protocol.ErrorCodeUnavailable || output.Kind != "" || calls != 1 {
				t.Fatalf("invalid source cursor escaped: output=%+v calls=%d err=%v", output, calls, err)
			}
		})
	}
}

func TestResourceProjectionRetainsOnlyUnavailableContent(t *testing.T) {
	for _, code := range []protocol.ErrorCode{protocol.ErrorCodeUnavailable, protocol.ErrorCodeBusy, protocol.ErrorCodeCancelled, protocol.ErrorCodeOutcomeUnknown} {
		if !resourceCacheAllowed(&protocol.Fault{Code: code}) {
			t.Fatalf("unavailable source %s discarded its projection", code)
		}
	}
	for _, code := range []protocol.ErrorCode{protocol.ErrorCodePermissionDenied, protocol.ErrorCodeNotFound, protocol.ErrorCodeExpired} {
		if resourceCacheAllowed(&protocol.Fault{Code: code}) {
			t.Fatalf("revoked source %s retained private content", code)
		}
	}
}

func TestStorageControllerCatalogDoesNotCallBackToController(t *testing.T) {
	node := nodeFixture(t)
	controller := protocol.NodeID{46}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		if err := store.PairLinkPeer(t.Context(), db, controller, link.DirectionController); err != nil {
			return err
		}
		user, err := store.CallerUser(t.Context(), db, node.ID(), controller)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(t.Context(), "UPDATE users SET controller=?1 WHERE id=?2", controller[:], user.ID.String())
		return err
	}); err != nil {
		t.Fatal(err)
	}
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "list_knowledge_collections"})
	response := dispatchFixture(t, node, controller, request)
	if response.Fault != nil || response.Output.Kind != "knowledge_collections" {
		t.Fatalf("storage read was sent back to its controller: %+v", response)
	}
}

func (transport *resourceCatalogTransport) Target() protocol.NodeID { return transport.target }
func (transport *resourceCatalogTransport) Dispatch(context.Context, protocol.Request) (link.Admission, error) {
	return link.Admission{}, resourceUnavailable()
}
func (transport *resourceCatalogTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, resourceUnavailable()
}
func (transport *resourceCatalogTransport) Subscribe(context.Context, protocol.Topic) (link.Subscription, error) {
	return nil, resourceUnavailable()
}
func (transport *resourceCatalogTransport) WatchConnection(context.Context, func(link.ConnectionInfo)) error {
	return nil
}
func (transport *resourceCatalogTransport) Resource(_ context.Context, call protocol.ResourceCall) (link.Admission, error) {
	transport.calls = append(transport.calls, call)
	output, err := transport.read(call)
	if err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	completion <- link.Response{Output: output}
	close(completion)
	return link.Admission{Receipt: protocol.Receipt{ID: call.Request.ID, Durable: call.Request.Command.Durable()}, Completion: completion}, nil
}

func resourceCatalogFixture(t *testing.T, node *Node, peers map[protocol.NodeID]*client.Client) context.Context {
	t.Helper()
	followers := &configurationFollowRuntime{peers: map[protocol.NodeID]*configurationFollowPeer{}}
	for id, remote := range peers {
		followers.peers[id] = &configurationFollowPeer{client: remote}
	}
	configurationFollowers.Store(node, followers)
	t.Cleanup(func() { configurationFollowers.Delete(node) })
	var user protocol.User
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		var err error
		user, err = store.LocalUser(t.Context(), db)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return store.WithUser(t.Context(), user)
}

func TestResourceCatalogKeepsUnavailableSources(t *testing.T) {
	node := nodeFixture(t)
	remoteID := protocol.NodeID{44}
	remote := client.New(&resourceCatalogTransport{target: remoteID})
	t.Cleanup(func() { _ = remote.Close() })
	ctx := resourceCatalogFixture(t, node, map[protocol.NodeID]*client.Client{remoteID: remote})
	previous := protocol.Project{ID: protocol.NewProjectID(), Name: "Offline project"}
	resourceProjections.Store(resourceProjectionKey{node, remoteID}, &resourceProjection{source: protocol.ResourceSource{Projects: []protocol.Project{previous}}})
	t.Cleanup(func() { resourceProjections.Delete(resourceProjectionKey{node, remoteID}) })
	catalog, err := node.resourceCatalog(ctx)
	if err != nil || len(catalog.Sources) != 2 || catalog.Entry != node.ID() {
		t.Fatalf("catalog: %+v: %v", catalog, err)
	}
	for _, source := range catalog.Sources {
		if source.Route.Entry != node.ID() || source.Route.Target != source.Node {
			t.Fatalf("source lost its entry: %+v", source)
		}
		if source.Node == remoteID && (source.Fault == nil || source.Fault.Code != protocol.ErrorCodeUnavailable || len(source.Projects) != 1 || source.Projects[0].ID != previous.ID) {
			t.Fatalf("offline source silently disappeared: %+v", source)
		}
	}
	var foreign protocol.User
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		foreign, err = store.CreateUser(ctx, db, "Other user")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	catalog, err = node.resourceCatalog(store.WithUser(ctx, foreign))
	if err != nil || len(catalog.Sources) != 1 {
		t.Fatalf("other user's host view crossed controller identities: %+v: %v", catalog, err)
	}
}

func TestKnowledgeTargetMustMatchOriginalRequest(t *testing.T) {
	node := nodeFixture(t)
	foreign := protocol.NodeID{45}
	definition := protocol.DefaultKnowledgeDefinition()
	definition.Name = "Other host"
	definition.Source = protocol.KnowledgeSource{Kind: "directory", Data: &protocol.KnowledgeSourceDirectoryPayload{Node: &foreign, Path: t.TempDir()}}
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{Node: &foreign, Definition: definition}})
	before, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := node.Dispatch(t.Context(), node.ID(), request); err == nil || resourceFault(err).Code != protocol.ErrorCodeWrongTarget {
		t.Fatalf("wrong-target request was rewritten: %v", err)
	}
	after, err := json.Marshal(request)
	if err != nil || string(after) != string(before) {
		t.Fatal("resource routing changed the captured request")
	}
}
