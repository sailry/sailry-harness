package node

import (
	"context"
	"database/sql"
	"sync"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// resourceObservers owns the direct-host clients that refresh the resource catalog.
var resourceObservers sync.Map

type resourceObserverPeer struct {
	cancel         context.CancelFunc
	reauthenticate chan struct{}
	client         *client.Client
}
type resourceObserverRuntime struct {
	mu    sync.Mutex
	peers map[protocol.NodeID]*resourceObserverPeer
}

// startResourceObservers watches authenticated direct hosts independently.
func (node *Node) startResourceObservers() error {
	if node.network == nil {
		return nil
	}
	runtime := &resourceObserverRuntime{peers: map[protocol.NodeID]*resourceObserverPeer{}}
	if _, loaded := resourceObservers.LoadOrStore(node, runtime); loaded {
		return nil
	}
	err := node.tasks.Submit(func(context.Context) {
		ctx := node.ctx
		defer resourceObservers.Delete(node)
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
				node.refreshResourceObservers(ctx, runtime)
			}
		}
	})
	if err != nil {
		resourceObservers.Delete(node)
	}
	return err
}

// Only an accepted invitation restores authority after a permanent observation
// failure. Ordinary peer metadata and connection changes do not grant it.
func (node *Node) reauthenticateResourceHost(host protocol.NodeID) {
	value, ok := resourceObservers.Load(node)
	if !ok {
		return
	}
	runtime := value.(*resourceObserverRuntime)
	runtime.mu.Lock()
	defer runtime.mu.Unlock()
	if peer := runtime.peers[host]; peer != nil {
		select {
		case peer.reauthenticate <- struct{}{}:
		default:
		}
	}
}

func resourceObservationTransient(err error) bool {
	fault := resourceFault(err)
	return fault != nil && (fault.Code == protocol.ErrorCodeUnavailable || fault.Code == protocol.ErrorCodeBusy)
}

func (node *Node) refreshResourceObservers(ctx context.Context, runtime *resourceObserverRuntime) {
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
		peer := &resourceObserverPeer{cancel: cancel, reauthenticate: make(chan struct{}, 1), client: client.New(remote)}
		runtime.peers[address.ID] = peer
		address := address
		if err := node.tasks.Submit(func(owner context.Context) {
			observer := peer.client
			defer observer.Close()
			cancelWatch := observer.OnChange(func() {
				node.notifyResourceCatalog(address.ID, observer)
			})
			defer cancelWatch()
			err := observer.Start(scoped)
			if scoped.Err() != nil {
				return
			}
			// Start retains an observation owner only after success or a
			// transient failure. Refresh can also restart a terminated owner.
			started := err == nil || resourceObservationTransient(err)
			for {
				select {
				case <-scoped.Done():
					return
				case <-peer.reauthenticate:
					if started {
						_ = observer.Refresh(scoped)
					} else {
						err = observer.Start(scoped)
						started = err == nil || resourceObservationTransient(err)
					}
				}
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
// the resource directory's observation lifecycle. Callers must not close them.
func (node *Node) controllerClients() map[protocol.NodeID]*client.Client {
	result := map[protocol.NodeID]*client.Client{}
	value, ok := resourceObservers.Load(node)
	if !ok {
		return result
	}
	runtime := value.(*resourceObserverRuntime)
	runtime.mu.Lock()
	defer runtime.mu.Unlock()
	for id, peer := range runtime.peers {
		result[id] = peer.client
	}
	return result
}
