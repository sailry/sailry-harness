package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"sync"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// configurationFollowers is runtime lifecycle state only. Canonical follow
// policy, reconciliation versions and pending request identities live in Store.
var configurationFollowers sync.Map

type configurationFollowPeer struct {
	cancel         context.CancelFunc
	wake           chan struct{}
	reauthenticate chan struct{}
	client         *client.Client
	remote         *link.Remote
}
type configurationFollowRuntime struct {
	wake  chan struct{}
	mu    sync.Mutex
	peers map[protocol.NodeID]*configurationFollowPeer
}

// StartConfigurationFollow watches authenticated direct host relationships.
// Each host reconciles serially, so an unavailable host cannot block the rest.
func (node *Node) StartConfigurationFollow() error {
	if node.network == nil {
		return nil
	}
	runtime := &configurationFollowRuntime{wake: make(chan struct{}, 1), peers: map[protocol.NodeID]*configurationFollowPeer{}}
	if _, loaded := configurationFollowers.LoadOrStore(node, runtime); loaded {
		return nil
	}
	err := node.tasks.Submit(func(context.Context) {
		ctx := node.ctx
		defer configurationFollowers.Delete(node)
		changed := make(chan struct{}, 1)
		wake := func() {
			select {
			case changed <- struct{}{}:
			default:
			}
		}
		if err := node.tasks.Submit(func(owner context.Context) { _ = node.network.WatchPeers(ctx, wake) }); err != nil {
			return
		}
		wake()
		defer func() {
			runtime.mu.Lock()
			defer runtime.mu.Unlock()
			for id, peer := range runtime.peers {
				peer.cancel()
				node.invalidateResourceProjection(id)
			}
		}()
		for {
			select {
			case <-ctx.Done():
				return
			case <-changed:
				node.refreshConfigurationFollowPeers(ctx, runtime)
			case <-runtime.wake:
				runtime.mu.Lock()
				for _, peer := range runtime.peers {
					select {
					case peer.wake <- struct{}{}:
					default:
					}
				}
				runtime.mu.Unlock()
			}
		}
	})
	if err != nil {
		configurationFollowers.Delete(node)
	}
	return err
}
func (node *Node) NotifyConfigurationFollow(changes []protocol.Event) {
	relevant := false
	for _, change := range changes {
		relevant = relevant || store.ConfigurationChanged(change.Kind) || change.Kind == "configuration_follow_changed" || change.Kind == "plugin_values_changed"
	}
	if !relevant {
		return
	}
	value, ok := configurationFollowers.Load(node)
	if !ok {
		return
	}
	runtime := value.(*configurationFollowRuntime)
	select {
	case runtime.wake <- struct{}{}:
	default:
	}
}

// Only an accepted invitation restores authority after a permanent observation
// failure. Ordinary peer metadata and connection changes do not grant it.
func (node *Node) reauthenticateConfigurationHost(host protocol.NodeID) {
	value, ok := configurationFollowers.Load(node)
	if !ok {
		return
	}
	runtime := value.(*configurationFollowRuntime)
	runtime.mu.Lock()
	defer runtime.mu.Unlock()
	if peer := runtime.peers[host]; peer != nil {
		select {
		case peer.reauthenticate <- struct{}{}:
		default:
		}
	}
}

func configurationObservationTransient(err error) bool {
	fault := resourceFault(err)
	return fault != nil && (fault.Code == protocol.ErrorCodeUnavailable || fault.Code == protocol.ErrorCodeBusy)
}

func (node *Node) refreshConfigurationFollowPeers(ctx context.Context, runtime *configurationFollowRuntime) {
	peers, err := node.network.Peers(ctx)
	if err != nil {
		return
	}
	relationships, err := node.network.Relationships(ctx)
	if err != nil {
		return
	}
	direct := map[protocol.NodeID]bool{}
	for _, peer := range relationships {
		direct[peer.ID] = peer.Direct && peer.DirectDirection.AllowsOutbound() && !peer.Removed
	}
	// Restore projections independently of Client startup, and drop entries for
	// connections explicitly removed while this Node was stopped.
	var local protocol.User
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		local, err = store.LocalUser(ctx, db)
		return err
	}); err != nil {
		return
	}
	owner := store.WithUser(ctx, local)
	allowed := []protocol.NodeID{}
	for id, available := range direct {
		if available {
			allowed = append(allowed, id)
		}
	}
	var removed []protocol.NodeID
	if err := node.profile.Store.Node.Write(owner, func(db *sql.Conn) error {
		var err error
		removed, err = store.PruneResourceCatalogSources(owner, db, node.ID(), allowed)
		if err != nil || len(removed) == 0 {
			return err
		}
		_, err = store.RecordEvents(owner, db, node.ID(), []protocol.Event{{Kind: "resource_catalog_changed"}})
		return err
	}); err != nil {
		return
	}
	for _, id := range removed {
		node.invalidateResourceProjection(id)
	}
	if len(removed) != 0 {
		node.publish()
	}
	seen := map[protocol.NodeID]bool{}
	runtime.mu.Lock()
	defer runtime.mu.Unlock()
	for _, address := range peers {
		if !direct[address.ID] || !node.network.Direction(address.ID).AllowsOutbound() {
			continue
		}
		seen[address.ID] = true
		if _, exists := runtime.peers[address.ID]; exists {
			continue
		}
		scoped, cancel := context.WithCancel(ctx)
		remote := node.network.Remote(address)
		peer := &configurationFollowPeer{cancel: cancel, wake: make(chan struct{}, 1), reauthenticate: make(chan struct{}, 1), remote: remote, client: client.New(remote)}
		runtime.peers[address.ID] = peer
		address := address
		if err := node.tasks.Submit(func(owner context.Context) {
			remote, observer := peer.remote, peer.client
			defer observer.Close()
			cancelWatch := observer.OnChange(func() {
				node.notifyResourceCatalog(address.ID, observer)
				select {
				case peer.wake <- struct{}{}:
				default:
				}
			})
			defer cancelWatch()
			// Reconnects and explicit retries wake reconciliation without waiting
			// for another controller edit.
			err := observer.Start(scoped)
			if scoped.Err() != nil {
				return
			}
			// Start retains an observation owner only after success or a
			// transient failure. Refresh can also restart a terminated owner.
			started := err == nil || configurationObservationTransient(err)
			if err := node.tasks.Submit(func(owner context.Context) {
				connected := false
				_ = remote.WatchConnection(scoped, func(info link.ConnectionInfo) {
					online := info.Route != nil
					if online && !connected {
						select {
						case peer.wake <- struct{}{}:
						default:
						}
					}
					connected = online
				})
			}); err != nil {
				return
			}
			for {
				select {
				case <-scoped.Done():
					return
				case <-peer.reauthenticate:
					if started {
						_ = observer.Refresh(scoped)
					} else {
						err = observer.Start(scoped)
						started = err == nil || configurationObservationTransient(err)
					}
				case <-peer.wake:
				}
				node.reconcileConfigurationHost(scoped, address, true)
			}
		}); err != nil {
			cancel()
			delete(runtime.peers, address.ID)
		}
	}
	for id, peer := range runtime.peers {
		if !seen[id] {
			peer.cancel()
			node.invalidateResourceProjection(id)
			delete(runtime.peers, id)
		}
	}
}

// controllerClients reuses the authenticated direct-host observers owned by
// the controller's reconciliation lifecycle. Callers must not close them.
func (node *Node) controllerClients() map[protocol.NodeID]*client.Client {
	result := map[protocol.NodeID]*client.Client{}
	value, ok := configurationFollowers.Load(node)
	if !ok {
		return result
	}
	runtime := value.(*configurationFollowRuntime)
	runtime.mu.Lock()
	defer runtime.mu.Unlock()
	for id, peer := range runtime.peers {
		result[id] = peer.client
	}
	return result
}

func configurationRemote(ctx context.Context, transport link.Transport, request protocol.Request) (protocol.Output, error) {
	accepted, err := transport.Dispatch(ctx, request)
	if err != nil {
		return protocol.Output{}, err
	}
	select {
	case <-ctx.Done():
		return protocol.Output{}, ctx.Err()
	case result, ok := <-accepted.Completion:
		if !ok {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "configuration delivery is unconfirmed"}
		}
		if result.Fault != nil {
			return protocol.Output{}, result.Fault
		}
		return result.Output, nil
	}
}

func (node *Node) reconcileConfigurationHost(ctx context.Context, address link.EndpointAddress, discover bool) {
	relationships, err := node.network.Relationships(ctx)
	if err != nil {
		return
	}
	if !slices.ContainsFunc(relationships, func(peer link.PeerRelationship) bool {
		return peer.ID == address.ID && peer.Direct && peer.DirectDirection.AllowsOutbound() && !peer.Removed
	}) || !node.network.Direction(address.ID).AllowsOutbound() {
		return
	}
	remote := node.network.Remote(address)
	var local protocol.User
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error { var err error; local, err = store.LocalUser(ctx, db); return err }); err != nil {
		return
	}
	ctx = store.WithUser(ctx, local)
	failed := false
	fail := func(cause error) {
		failed = true
		var failure *protocol.Fault
		if !errors.As(cause, &failure) {
			var wrapped *link.FaultError
			if errors.As(cause, &wrapped) {
				value := wrapped.Fault
				failure = &value
			} else {
				failure = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "configuration host is unavailable"}
			}
		}
		_ = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error { return store.RecordConfigurationTargetFailure(ctx, db, address.ID, failure) })
	}
	defer func() {
		// Personal data has its own durable requests and must converge even
		// when an unrelated configuration category cannot be published.
		if err := node.reconcilePersonalKV(ctx, remote); err != nil {
			fail(err)
			return
		}
		if !failed {
			_ = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error { return store.RecordConfigurationTargetFailure(ctx, db, address.ID, nil) })
		}
	}()
	bindSource := false
	if discover {
		output, err := configurationRemote(ctx, remote, protocol.NewRequest(address.ID, protocol.Command{Kind: "configuration_follow", Data: &protocol.ConfigurationFollowCommand{Action: "read"}}))
		if err != nil {
			fail(err)
			return
		}
		var status protocol.ConfigurationFollowStatus
		if body, err := json.Marshal(output.Data); err != nil || json.Unmarshal(body, &status) != nil || status.User == nil {
			return
		}
		// Rebind a retained legacy copy with a metadata-only apply even when
		// every record already matches the controller's manifest.
		bindSource = status.Source == nil
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			return store.EnsureConfigurationTarget(ctx, db, address.ID)
		}); err != nil {
			return
		}
	}
	var status protocol.ConfigurationFollowStatus
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		status, err = store.ReadConfigurationFollow(ctx, db)
		return err
	}); err != nil {
		return
	}
	var target *protocol.ConfigurationFollowTarget
	for _, value := range status.Targets {
		if value.Host == address.ID {
			copied := value
			target = &copied
			break
		}
	}
	if status.Source != nil && *status.Source != node.ID() {
		return
	}
	if target == nil || !target.Configuration {
		return
	}
	declarations := map[string]protocol.PluginMcpConfiguration{}
	var infos []protocol.PluginInfo
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error { var err error; infos, err = store.ReadPluginInfos(ctx, db); return err }); err != nil {
		fail(err)
		return
	}
	for _, info := range infos {
		if len(info.Mcp) == 0 {
			continue
		}
		value, err := node.declaredPluginMCP(ctx, plugins.PackageReference(info))
		if err != nil {
			fail(err)
			return
		}
		declarations[info.Summary.Name] = value
	}
	for _, category := range store.ConfigurationCategories {
		output, err := configurationRemote(ctx, remote, protocol.NewRequest(address.ID, protocol.Command{Kind: "configuration_follow", Data: &protocol.ConfigurationFollowCommand{Action: "manifest", Source: pointerNode(node.ID()), Category: category}}))
		if err != nil {
			fail(err)
			return
		}
		var known protocol.ConfigurationManifest
		body, err := json.Marshal(output.Data)
		if err != nil || json.Unmarshal(body, &known) != nil {
			fail(errors.New("invalid remote manifest"))
			return
		}
		var request protocol.Request
		unchanged := false
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			manifest, err := store.ExportConfigurationManifest(ctx, db, node.ID(), address.ID, category, target.Secrets, declarations)
			if err != nil {
				return err
			}
			var pending bool
			if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM configuration_follow_deliveries WHERE user=?1 AND host=?2 AND category=?3)", local.ID.String(), address.ID[:], category).Scan(&pending); err != nil {
				return err
			}
			if !pending && !bindSource && known.Revision == manifest.Revision && len(known.Records) == len(manifest.Records) {
				matches := true
				have := map[string]string{}
				for _, record := range known.Records {
					have[record.ID] = record.Digest
				}
				for _, record := range manifest.Records {
					matches = matches && have[record.ID] == record.Digest
				}
				if matches {
					unchanged = true
					return nil
				}
			}
			have := map[string]protocol.ConfigurationFollowRecord{}
			for _, record := range known.Records {
				have[record.ID] = record
			}
			for index, record := range manifest.Records {
				if previous, ok := have[record.ID]; ok && previous.Digest == record.Digest && previous.Revision == record.Revision {
					manifest.Records[index].Body = nil
					manifest.Records[index].Secret = nil
				}
			}
			request, err = store.ConfigurationFollowDelivery(ctx, db, address.ID, manifest)
			return err
		}); err != nil {
			fail(err)
			return
		}
		if unchanged {
			continue
		}
		var delivery protocol.ConfigurationFollowCommand
		if value, ok := dispatchPayload[protocol.ConfigurationFollowCommand](request.Command.Data); ok {
			delivery = value
		}
		if delivery.Manifest != nil {
			if err := node.prepareConfigurationPackages(ctx, remote, *delivery.Manifest); err != nil {
				fail(err)
				return
			}
		}
		err = node.deliverConfigurationSnapshot(ctx, remote, request)
		if err != nil {
			var definitive *protocol.Fault
			if errors.As(err, &definitive) && definitive.Code != protocol.ErrorCodeOutcomeUnknown && definitive.Code != protocol.ErrorCodeUnavailable && definitive.Code != protocol.ErrorCodeCancelled && definitive.Code != protocol.ErrorCodeBusy {
				_ = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
					return store.CompleteConfigurationFollowDelivery(ctx, db, address.ID, category, request.ID)
				})
			}
			fail(err)
			return
		}
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			return store.CompleteConfigurationFollowDelivery(ctx, db, address.ID, category, request.ID)
		}); err != nil {
			fail(err)
			return
		}
	}
}

func pointerNode(value protocol.NodeID) *protocol.NodeID { return &value }

func SupportsConfigurationFollow(kind string) bool { return kind == "configuration_follow" }

func (node *Node) ReadConfigurationFollow(ctx context.Context, caller protocol.NodeID, request protocol.Request) (protocol.Output, error) {
	input, ok := dispatchPayload[protocol.ConfigurationFollowCommand](request.Command.Data)
	if !ok {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "configuration follow command is invalid"}
	}
	if input.Action == "cache_package" || input.Action == "package_state" {
		return node.readConfigurationPackage(ctx, caller, input)
	}
	if input.Action == "apply_page" {
		return node.applyConfigurationSnapshotPage(ctx, caller, request, input.Page)
	}
	var output protocol.Output
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		switch input.Action {
		case "read":
			value, err := store.ReadConfigurationFollow(ctx, db)
			output = protocol.Output{Kind: "configuration_follow", Data: value}
			return err
		case "manifest":
			if input.Source == nil || *input.Source != caller {
				return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "configuration manifest belongs to another source"}
			}
			value, err := store.ReadConfigurationManifest(ctx, db, caller, input.Category)
			output = protocol.Output{Kind: "configuration_manifest", Data: value}
			return err
		case "kv_pending":
			value, err := store.ReadPersonalKVPending(ctx, db, node.ID(), caller, input.KVRead)
			output = protocol.Output{Kind: "personal_kv_pending", Data: value}
			return err
		case "snapshot_state":
			if input.Page == nil {
				return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "configuration snapshot header is required"}
			}
			value, err := store.ReadConfigurationSnapshot(ctx, db, node.ID(), caller, *input.Page)
			output = protocol.Output{Kind: "configuration_snapshot", Data: value}
			return err
		}
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "configuration follow read action is invalid"}
	})
	return output, err
}

// ExecuteConfigurationFollow participates in Node's existing command transaction
// and request ledger. The coordinator records returned events before commit.
func ExecuteConfigurationFollow(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, command protocol.Command) (protocol.Output, []protocol.Event, error) {
	input, ok := dispatchPayload[protocol.ConfigurationFollowCommand](command.Data)
	if !ok {
		return protocol.Output{}, nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "configuration follow command is invalid"}
	}
	var events []protocol.Event
	var err error
	switch input.Action {
	case "kv_ack":
		if input.KVReceipt == nil {
			return protocol.Output{}, nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "personal data receipt is required"}
		}
		events, err = store.AcknowledgePersonalKV(ctx, db, node, caller, *input.KVReceipt)
	case "kv_snapshot":
		if input.KVSnapshot == nil {
			return protocol.Output{}, nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "personal data snapshot page is required"}
		}
		events, err = store.StagePersonalKVSnapshot(ctx, db, node, caller, *input.KVSnapshot)
	case "apply":
		if input.Manifest == nil {
			return protocol.Output{}, nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "configuration manifest is required"}
		}
		events, err = store.ApplyConfigurationManifest(ctx, db, node, caller, *input.Manifest)
	case "sync":
	default:
		err = &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "configuration follow action is not supported"}
	}
	if err != nil {
		return protocol.Output{}, nil, err
	}
	status, err := store.ReadConfigurationFollow(ctx, db)
	if len(events) == 0 {
		events = []protocol.Event{{Kind: "configuration_follow_changed", Data: status}}
	}
	return protocol.Output{Kind: "configuration_follow", Data: status}, events, err
}
