package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

// The real source Node owns projects and emits its normal events. Only its
// transport is scripted, so outages and truncated snapshots use no network or
// user profile and the controller still follows the production observer path.
type resourceDirectoryTransport struct {
	link.Transport
	source  *Node
	user    protocol.User
	mu      sync.Mutex
	ctx     context.Context
	cancel  context.CancelFunc
	offline bool
	partial bool
	fault   *protocol.Fault
	// A blocked metadata response keeps a connected source unconfirmed until
	// the fixture explicitly completes that observation.
	knowledgeGate    <-chan struct{}
	knowledgeStarted chan<- struct{}
}

func newResourceDirectoryTransport(t *testing.T, source *Node) *resourceDirectoryTransport {
	t.Helper()
	ctx, cancel := context.WithCancel(t.Context())
	transport := &resourceDirectoryTransport{Transport: link.NewLocal(source.ID(), source.ID(), source), source: source, ctx: ctx, cancel: cancel}
	if err := source.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		transport.user, err = store.LocalUser(ctx, db)
		return err
	}); err != nil {
		cancel()
		t.Fatal(err)
	}
	t.Cleanup(cancel)
	return transport
}

func (transport *resourceDirectoryTransport) setOffline(offline, partial bool) {
	transport.mu.Lock()
	transport.cancel()
	transport.ctx, transport.cancel = context.WithCancel(transport.source.ctx)
	transport.offline, transport.partial = offline, partial
	transport.mu.Unlock()
}

func (transport *resourceDirectoryTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	return transport.Transport.Dispatch(store.WithUser(ctx, transport.user), request)
}

func (transport *resourceDirectoryTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	ctx = store.WithUser(ctx, transport.user)
	transport.mu.Lock()
	offline, partial, connection := transport.offline, transport.partial, transport.ctx
	transport.mu.Unlock()
	if offline {
		return nil, resourceUnavailable()
	}
	if partial {
		snapshot, err := transport.source.Snapshot(ctx)
		if err != nil {
			return nil, err
		}
		body, err := json.Marshal(snapshot)
		if err != nil {
			return nil, err
		}
		frames := []wire.Reply{
			{Kind: wire.ReplySnapshotStart, SnapshotStart: &wire.SnapshotHeader{Delivery: wire.DeliveryUpdate, Node: snapshot.Node, Cursor: snapshot.Cursor, Bytes: uint64(len(body))}},
			{Kind: wire.ReplySnapshotPage, SnapshotPage: &wire.SnapshotPage{Index: 0, Data: string(body[:len(body)/2])}},
		}
		return &resourceDirectorySubscription{connection: connection, frames: frames}, nil
	}
	subscription, err := transport.Transport.Subscribe(ctx, topic)
	if err != nil {
		return nil, err
	}
	return &resourceDirectorySubscription{connection: connection, subscription: subscription}, nil
}

func (transport *resourceDirectoryTransport) Resource(ctx context.Context, call protocol.ResourceCall) (link.Admission, error) {
	transport.mu.Lock()
	offline, fault, gate, started := transport.offline, transport.fault, transport.knowledgeGate, transport.knowledgeStarted
	transport.mu.Unlock()
	if offline {
		return link.Admission{}, resourceUnavailable()
	}
	if call.Request.Command.Kind == "list_knowledge_collections" && gate != nil {
		select {
		case started <- struct{}{}:
		default:
		}
		select {
		case <-gate:
		case <-ctx.Done():
			return link.Admission{}, ctx.Err()
		}
	}
	if fault != nil {
		return link.Admission{}, fault
	}
	return transport.source.Resource(store.WithUser(ctx, transport.user), transport.source.ID(), call)
}

type resourceDirectorySubscription struct {
	connection   context.Context
	subscription link.Subscription
	frames       []wire.Reply
}

func (subscription *resourceDirectorySubscription) Next(ctx context.Context) (protocol.Update, error) {
	if subscription.subscription == nil {
		var assembly wire.Assembly
		for _, frame := range subscription.frames {
			if completed, err := assembly.Accept(frame); err != nil || completed != nil {
				return protocol.Update{}, errors.New("truncated snapshot unexpectedly completed")
			}
		}
		return protocol.Update{}, resourceUnavailable()
	}
	scoped, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(subscription.connection, cancel)
	defer stop()
	defer cancel()
	update, err := subscription.subscription.Next(scoped)
	if subscription.connection.Err() != nil && ctx.Err() == nil {
		return protocol.Update{}, resourceUnavailable()
	}
	return update, err
}

func (subscription *resourceDirectorySubscription) Close() error {
	if subscription.subscription != nil {
		return subscription.subscription.Close()
	}
	return nil
}

func watchResourceDirectory(t *testing.T, controller, source *Node) (*resourceDirectoryTransport, *client.Client, context.Context, link.Subscription) {
	t.Helper()
	transport := newResourceDirectoryTransport(t, source)
	remote := client.New(transport)
	t.Cleanup(func() { _ = remote.Close() })
	ctx, cancel := context.WithTimeout(resourceCatalogFixture(t, controller, map[protocol.NodeID]*client.Client{source.ID(): remote}), 15*time.Second)
	t.Cleanup(cancel)
	owner, err := controller.Subscribe(ctx, controller.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = owner.Close() })
	release := remote.OnChange(func() { controller.notifyResourceCatalog(source.ID(), remote) })
	t.Cleanup(release)
	t.Cleanup(func() { controller.invalidateResourceProjection(source.ID()) })
	if err := remote.Start(ctx); err != nil {
		t.Fatal(err)
	}
	return transport, remote, ctx, owner
}

func waitResourceDirectory(t *testing.T, node *Node, ctx context.Context, changes link.Subscription, ready func([]protocol.ResourceSource) bool) []protocol.ResourceSource {
	t.Helper()
	for {
		var values []protocol.ResourceSource
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			values, err = store.ReadResourceCatalogSources(ctx, db, node.ID())
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if ready(values) {
			return values
		}
		if _, err := changes.Next(ctx); err != nil {
			t.Fatal(err)
		}
	}
}

func TestResourceDirectoryObservesCreationAndRestoresOfflineAfterRestart(t *testing.T) {
	controller, source := nodeFixture(t), nodeFixture(t)
	transport, remote, ctx, changes := watchResourceDirectory(t, controller, source)
	waitResourceDirectory(t, controller, ctx, changes, func(values []protocol.ResourceSource) bool { return len(values) == 1 })
	// No catalog or page read precedes creation. Its normal source event must
	// cause the shared observer to persist the newly registered project.
	created := commandFixture(t, source, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Remote project", Path: t.TempDir()}}).Data.(protocol.Project)
	values := waitResourceDirectory(t, controller, ctx, changes, func(values []protocol.ResourceSource) bool {
		return len(values) == 1 && len(values[0].Projects) == 1 && values[0].Projects[0].ID == created.ID
	})
	if len(values[0].Worktrees) != 1 || values[0].Worktrees[0].Project == nil || *values[0].Worktrees[0].Project != created.ID || values[0].LastSyncedAtMs == nil {
		t.Fatalf("project event lost its worktree or confirmed timestamp: %+v", values)
	}
	confirmed := *values[0].LastSyncedAtMs
	transport.setOffline(true, false)
	if err := remote.Refresh(ctx); err == nil {
		t.Fatal("offline source established an observation")
	}
	if err := remote.Close(); err != nil {
		t.Fatal(err)
	}
	catalog, err := controller.resourceCatalog(ctx)
	if err != nil || len(catalog.Sources) != 2 || len(catalog.Sources[1].Projects) != 1 || catalog.Sources[1].Fault == nil || catalog.Sources[1].LastSyncedAtMs == nil {
		t.Fatalf("offline source lost its project: %+v: %v", catalog, err)
	}
	confirmed = *catalog.Sources[1].LastSyncedAtMs
	path, identity := controller.profile.Ownership.Path, controller.ID()
	if err := changes.Close(); err != nil {
		t.Fatal(err)
	}
	configurationFollowers.Delete(controller)
	controller.invalidateResourceProjection(source.ID())
	if err := controller.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	restarted, err := startFixtureNode(t.Context(), path)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = restarted.Stop(context.Background()) })
	// There is deliberately no remote Client in this freshly started Node.
	catalog, err = restarted.resourceCatalog(t.Context())
	if err != nil || restarted.ID() != identity || len(catalog.Sources) != 2 {
		t.Fatalf("cold directory did not restore before connecting: %+v: %v", catalog, err)
	}
	previous := catalog.Sources[1]
	if previous.Node != source.ID() || previous.Fault == nil || len(previous.Projects) != 1 || previous.Projects[0].ID != created.ID || previous.LastSyncedAtMs == nil || *previous.LastSyncedAtMs != confirmed {
		t.Fatalf("cold directory lost source identity or last successful sync: %+v", previous)
	}
}

func TestResourceDirectoryRejectsPartialDeletionAndReconcilesFullReconnect(t *testing.T) {
	controller, source := nodeFixture(t), nodeFixture(t)
	created := commandFixture(t, source, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Remote project", Path: t.TempDir()}}).Data.(protocol.Project)
	transport, remote, ctx, changes := watchResourceDirectory(t, controller, source)
	waitResourceDirectory(t, controller, ctx, changes, func(values []protocol.ResourceSource) bool {
		return len(values) == 1 && len(values[0].Projects) == 1
	})
	transport.setOffline(true, false)
	_ = remote.Refresh(ctx)
	commandFixture(t, source, protocol.Command{Kind: "remove_project", Data: protocol.CommandRemoveProjectPayload{Expected: created}})
	transport.setOffline(false, true)
	if err := remote.Refresh(ctx); err == nil {
		t.Fatal("truncated source snapshot was published")
	}
	catalog, err := controller.resourceCatalog(ctx)
	if err != nil || len(catalog.Sources) != 2 || len(catalog.Sources[1].Projects) != 1 || catalog.Sources[1].Projects[0].ID != created.ID {
		t.Fatalf("partial deletion cleared the last complete directory: %+v: %v", catalog, err)
	}
	transport.setOffline(false, false)
	if err := remote.Refresh(ctx); err != nil {
		t.Fatal(err)
	}
	waitResourceDirectory(t, controller, ctx, changes, func(values []protocol.ResourceSource) bool {
		return len(values) == 1 && len(values[0].Projects) == 0 && len(values[0].Worktrees) == 0
	})
}

func TestResourceDirectoryRevocationClearsMemoryAndDisk(t *testing.T) {
	controller, source := nodeFixture(t), nodeFixture(t)
	commandFixture(t, source, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Remote project", Path: t.TempDir()}})
	transport, remote, ctx, changes := watchResourceDirectory(t, controller, source)
	waitResourceDirectory(t, controller, ctx, changes, func(values []protocol.ResourceSource) bool {
		return len(values) == 1 && len(values[0].Projects) == 1
	})
	transport.mu.Lock()
	transport.fault = &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "source access was revoked"}
	transport.mu.Unlock()
	controller.notifyResourceCatalog(source.ID(), remote)
	waitResourceDirectory(t, controller, ctx, changes, func(values []protocol.ResourceSource) bool { return len(values) == 0 })
	catalog, err := controller.resourceCatalog(ctx)
	if err != nil || len(catalog.Sources) != 2 || len(catalog.Sources[1].Projects) != 0 || len(catalog.Sources[1].Worktrees) != 0 || catalog.Sources[1].Fault == nil || catalog.Sources[1].Fault.Code != protocol.ErrorCodePermissionDenied {
		t.Fatalf("revocation retained a private projection: %+v: %v", catalog, err)
	}
	resourceProjections.Range(func(key, value any) bool {
		identity := key.(resourceProjectionKey)
		if identity.owner == controller && identity.source == source.ID() {
			state := value.(*resourceProjection)
			state.mu.Lock()
			defer state.mu.Unlock()
			if len(state.source.Projects) != 0 || len(state.source.Worktrees) != 0 {
				t.Error("revocation retained private resources in memory")
			}
		}
		return true
	})
}

func TestResourceDirectoryReconnectUpdatesExistingProject(t *testing.T) {
	controller, source := nodeFixture(t), nodeFixture(t)
	created := commandFixture(t, source, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Before reconnect", Path: t.TempDir()}}).Data.(protocol.Project)
	transport, remote, ctx, changes := watchResourceDirectory(t, controller, source)
	waitResourceDirectory(t, controller, ctx, changes, func(values []protocol.ResourceSource) bool {
		return len(values) == 1 && len(values[0].Projects) == 1 && values[0].Projects[0].Name == created.Name
	})
	transport.setOffline(true, false)
	_ = remote.Refresh(ctx)
	changed := commandFixture(t, source, protocol.Command{Kind: "update_project", Data: protocol.CommandUpdateProjectPayload{
		Expected: created, Name: "After reconnect", Path: created.Path, Appearance: created.Appearance,
	}}).Data.(protocol.Project)
	transport.setOffline(false, false)
	if err := remote.Refresh(ctx); err != nil {
		t.Fatal(err)
	}
	values := waitResourceDirectory(t, controller, ctx, changes, func(values []protocol.ResourceSource) bool {
		return len(values) == 1 && len(values[0].Projects) == 1 && values[0].Projects[0].Name == changed.Name
	})
	if values[0].Projects[0].ID != created.ID || len(values[0].Worktrees) != 1 {
		t.Fatalf("reconnect duplicated a resource or replaced its identity: %+v", values)
	}
	if err := controller.profile.Store.Node.Write(ctx, func(db *sql.Conn) error { return store.SetLinkTrust(ctx, db, source.ID(), false) }); err != nil {
		t.Fatal(err)
	}
	// An observation completing after removal must not resurrect its disk
	// cache. The source's actual project remains untouched.
	controller.notifyResourceCatalog(source.ID(), remote)
	if err := controller.forgetResourceCatalog(ctx, source.ID()); err != nil {
		t.Fatal(err)
	}
	waitResourceDirectory(t, controller, ctx, changes, func(values []protocol.ResourceSource) bool { return len(values) == 0 })
	snapshot := snapshotFixture(t, source)
	if len(snapshot.Projects) != 1 || snapshot.Projects[0].ID != created.ID {
		t.Fatalf("removing a controller connection removed the actual remote project: %+v", snapshot.Projects)
	}
}

func TestResourceDirectoryKeepsConnectedDiskSummariesUnavailableUntilMetadataIsConfirmed(t *testing.T) {
	controller, source := nodeFixture(t), nodeFixture(t)
	packageDir := t.TempDir()
	manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"fixture-reader","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["knowledge.read"]}}}`
	if err := os.WriteFile(filepath.Join(packageDir, "plugin.json"), []byte(manifest), 0o600); err != nil {
		t.Fatal(err)
	}
	info, err := source.plugins.manager.Install(t.Context(), packageDir, "", "fixture-reader")
	if err != nil || len(info.Issues) != 0 || info.Extension == nil {
		t.Fatalf("knowledge reader fixture is invalid: %+v: %v", info.Issues, err)
	}
	if err := source.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		_, err := store.InstallPlugin(t.Context(), db, info, 0, true)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	definition := protocol.DefaultKnowledgeDefinition()
	definition.Name, definition.Globs = "Source recipe", []string{"**/*.md"}
	definition.Tags, definition.SharedWith = []string{"reference"}, []string{"fixture-reader"}
	definition.Source = protocol.KnowledgeSource{Kind: "directory", Data: &protocol.KnowledgeSourceDirectoryPayload{Path: t.TempDir()}}
	target := source.ID()
	collection := commandFixture(t, source, protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{
		Node: &target, Definition: definition,
	}}).Data.(protocol.KnowledgeCollection)
	transport := newResourceDirectoryTransport(t, source)
	gate, started := make(chan struct{}), make(chan struct{}, 1)
	var release sync.Once
	t.Cleanup(func() { release.Do(func() { close(gate) }) })
	transport.knowledgeGate, transport.knowledgeStarted = gate, started
	transport.fault = resourceUnavailable()
	remote := client.New(transport)
	t.Cleanup(func() { _ = remote.Close() })
	ctx, cancel := context.WithTimeout(resourceCatalogFixture(t, controller, map[protocol.NodeID]*client.Client{source.ID(): remote}), 15*time.Second)
	t.Cleanup(cancel)
	t.Cleanup(func() { controller.invalidateResourceProjection(source.ID()) })
	if err := controller.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		return store.SaveResourceCatalogSource(ctx, db, controller.ID(), protocol.ResourceSource{
			Node: source.ID(), Route: protocol.RouteKey{Entry: controller.ID(), Target: source.ID()}, Knowledge: []protocol.KnowledgeCollection{collection},
		})
	}); err != nil {
		t.Fatal(err)
	}
	changes, err := controller.Subscribe(ctx, controller.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = changes.Close() })
	// No OnChange callback has confirmed the knowledge metadata. Establishing
	// the ordinary source snapshot alone must not enable writes from summaries.
	if err := remote.Start(ctx); err != nil || !remote.Connected() {
		t.Fatalf("source snapshot did not connect: %v", err)
	}
	catalog, err := controller.resourceCatalog(ctx)
	if err != nil || len(catalog.Sources) != 2 {
		t.Fatalf("cached directory was not readable: %+v: %v", catalog, err)
	}
	assertUnavailableSummary := func(value protocol.ResourceSource) {
		t.Helper()
		if value.Fault == nil || value.Fault.Code != protocol.ErrorCodeUnavailable || len(value.Knowledge) != 1 || value.Knowledge[0].ID != collection.ID || len(value.Knowledge[0].Definition.Globs) != 0 || len(value.Knowledge[0].Definition.SharedWith) != 0 {
			t.Fatalf("cached summary became an editable authoritative definition: %+v", value)
		}
	}
	assertUnavailableSummary(catalog.Sources[1])
	select {
	case <-started:
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	transport.mu.Lock()
	transport.knowledgeGate, transport.knowledgeStarted = nil, nil
	transport.mu.Unlock()
	release.Do(func() { close(gate) })
	user, _ := store.UserFromContext(ctx)
	waitProjection := func(ready func(protocol.ResourceSource) bool) {
		t.Helper()
		for {
			if value, exists := resourceProjections.Load(resourceProjectionKey{controller, source.ID(), user.ID}); exists {
				state := value.(*resourceProjection)
				state.mu.Lock()
				done := state.known && !state.removed && ready(state.source)
				state.mu.Unlock()
				if done {
					return
				}
			}
			if _, err := changes.Next(ctx); err != nil {
				t.Fatal(err)
			}
		}
	}
	waitProjection(func(value protocol.ResourceSource) bool { return value.Fault != nil })
	// A complete project snapshot can be saved during a transient knowledge
	// failure. That successful disk write must not clear the live metadata fault.
	catalog, err = controller.resourceCatalog(ctx)
	if err != nil || len(catalog.Sources) != 2 {
		t.Fatalf("transient metadata failure lost the summary: %+v: %v", catalog, err)
	}
	assertUnavailableSummary(catalog.Sources[1])
	transport.mu.Lock()
	transport.fault = nil
	transport.mu.Unlock()
	controller.notifyResourceCatalog(source.ID(), remote)
	waitProjection(func(value protocol.ResourceSource) bool {
		return value.Fault == nil && len(value.Knowledge) == 1 && slices.Equal(value.Knowledge[0].Definition.SharedWith, definition.SharedWith)
	})
	catalog, err = controller.resourceCatalog(ctx)
	if err != nil || len(catalog.Sources) != 2 {
		t.Fatalf("confirmed directory could not be read: %+v: %v", catalog, err)
	}
	confirmed := catalog.Sources[1]
	if confirmed.Fault != nil || len(confirmed.Knowledge) != 1 || !slices.Equal(confirmed.Knowledge[0].Definition.Globs, definition.Globs) || !slices.Equal(confirmed.Knowledge[0].Definition.SharedWith, definition.SharedWith) {
		t.Fatalf("confirmed source did not restore its complete definition: %+v", confirmed)
	}
}

func TestResourceDirectoryRevocationDiscardsInFlightSuccessfulCapture(t *testing.T) {
	controller, source := nodeFixture(t), nodeFixture(t)
	project := commandFixture(t, source, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Revoked private project", Path: t.TempDir()}}).Data.(protocol.Project)
	transport := newResourceDirectoryTransport(t, source)
	gate, started := make(chan struct{}), make(chan struct{}, 1)
	var release sync.Once
	t.Cleanup(func() { release.Do(func() { close(gate) }) })
	transport.knowledgeGate, transport.knowledgeStarted = gate, started
	remote := client.New(transport)
	t.Cleanup(func() { _ = remote.Close() })
	ctx, cancel := context.WithTimeout(resourceCatalogFixture(t, controller, map[protocol.NodeID]*client.Client{source.ID(): remote}), 15*time.Second)
	t.Cleanup(cancel)
	t.Cleanup(func() { controller.invalidateResourceProjection(source.ID()) })
	if err := remote.Start(ctx); err != nil {
		t.Fatal(err)
	}
	user, _ := store.UserFromContext(ctx)
	key := resourceProjectionKey{controller, source.ID(), user.ID}
	state := &resourceProjection{}
	resourceProjections.Store(key, state)
	previous := protocol.ResourceSource{Node: source.ID(), Route: protocol.RouteKey{Entry: controller.ID(), Target: source.ID()}, Projects: []protocol.Project{project}}
	if err := controller.saveResourceProjection(ctx, state, previous, true, 0); err != nil {
		t.Fatal(err)
	}
	state.mu.Lock()
	generation := state.generation
	state.mu.Unlock()
	completed := make(chan error, 1)
	go func() {
		captured, complete := controller.captureResourceSource(ctx, source.ID(), remote, previous)
		// This answer was already authenticated before the revocation and is
		// deliberately successful after it, as with a delayed network response.
		if captured.Fault != nil || !complete {
			completed <- errors.New("fixture did not produce a delayed successful capture")
			return
		}
		completed <- controller.saveResourceProjection(ctx, state, captured, complete, generation)
	}()
	select {
	case <-started:
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	if err := controller.revokeResourceCatalog(ctx, user.ID, source.ID(), &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "source access was revoked"}); err != nil {
		t.Fatal(err)
	}
	release.Do(func() { close(gate) })
	select {
	case err := <-completed:
		if err != nil {
			t.Fatal(err)
		}
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	// There is no second observer wake to clean up a bad write. The stale
	// generation itself must prevent both disk and memory resurrection.
	if err := controller.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		values, err := store.ReadResourceCatalogSources(ctx, db, controller.ID())
		if err == nil && len(values) != 0 {
			return errors.New("delayed successful response recreated the revoked disk cache")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	state.mu.Lock()
	defer state.mu.Unlock()
	if !state.known || state.generation == generation || state.source.Fault == nil || state.source.Fault.Code != protocol.ErrorCodePermissionDenied || len(state.source.Projects) != 0 || len(state.source.Worktrees) != 0 || len(state.source.Knowledge) != 0 {
		t.Fatalf("delayed successful response restored a revoked live projection: %+v", state.source)
	}
}
