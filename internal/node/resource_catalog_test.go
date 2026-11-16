package node

import (
	"context"
	"database/sql"
	"encoding/json"
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
