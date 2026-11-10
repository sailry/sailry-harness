package live

import (
	"context"
	"errors"
	"slices"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type networkPeer struct {
	address    link.EndpointAddress
	info       link.PeerInfo
	controller *client.Client
	connection link.ConnectionInfo
	online     bool
	inspecting bool
	err        error
	cancel     context.CancelFunc
	release    func()
}

type Network struct {
	local     *client.Client
	endpoint  *link.Link
	ctx       context.Context
	cancel    context.CancelFunc
	mu        sync.Mutex
	peers     map[protocol.NodeID]*networkPeer
	callbacks map[uint64]func()
	callback  uint64
	workers   sync.WaitGroup
	closed    bool
}

func NewNetwork(ctx context.Context, local *client.Client, endpoint *link.Link) *Network {
	ctx, cancel := context.WithCancel(ctx)
	network := &Network{local: local, endpoint: endpoint, ctx: ctx, cancel: cancel, peers: map[protocol.NodeID]*networkPeer{}, callbacks: map[uint64]func(){}}
	if endpoint != nil {
		network.workers.Add(2)
		go func() {
			defer network.workers.Done()
			_ = endpoint.WatchPeers(ctx, network.discover)
		}()
		go func() {
			defer network.workers.Done()
			ticker := time.NewTicker(10 * time.Second)
			defer ticker.Stop()
			for {
				select {
				case <-ctx.Done():
					return
				case <-ticker.C:
					network.discover()
				}
			}
		}()
	}
	return network
}

func (network *Network) onChange(callback func()) func() {
	network.mu.Lock()
	network.callback++
	token := network.callback
	network.callbacks[token] = callback
	network.mu.Unlock()
	return func() {
		network.mu.Lock()
		delete(network.callbacks, token)
		network.mu.Unlock()
	}
}

func (network *Network) changed() {
	network.mu.Lock()
	callbacks := make([]func(), 0, len(network.callbacks))
	for _, callback := range network.callbacks {
		callbacks = append(callbacks, callback)
	}
	network.mu.Unlock()
	for _, callback := range callbacks {
		callback()
	}
}

func (network *Network) snapshot() []networkPeer {
	network.mu.Lock()
	defer network.mu.Unlock()
	peers := make([]networkPeer, 0, len(network.peers))
	for _, peer := range network.peers {
		peers = append(peers, *peer)
	}
	slices.SortFunc(peers, func(first, second networkPeer) int {
		return slices.Compare(first.address.ID[:], second.address.ID[:])
	})
	return peers
}

func (network *Network) discover() {
	addresses, err := network.endpoint.Peers(network.ctx)
	if err != nil {
		return
	}
	present := map[protocol.NodeID]bool{}
	var removed []*networkPeer
	network.mu.Lock()
	if network.closed {
		network.mu.Unlock()
		return
	}
	for _, address := range addresses {
		present[address.ID] = true
		if network.peers[address.ID] == nil {
			network.peers[address.ID] = &networkPeer{address: address}
		} else {
			network.peers[address.ID].address = address
		}
	}
	for id, peer := range network.peers {
		if !present[id] {
			removed = append(removed, peer)
			delete(network.peers, id)
		}
	}
	network.mu.Unlock()
	for _, peer := range removed {
		network.closePeer(peer)
	}
	for _, address := range addresses {
		network.inspect(address.ID)
	}
	network.changed()
}

func (network *Network) inspect(id protocol.NodeID) {
	network.mu.Lock()
	peer := network.peers[id]
	if network.closed || peer == nil || peer.inspecting {
		network.mu.Unlock()
		return
	}
	peer.inspecting = true
	address := peer.address
	network.workers.Add(1)
	network.mu.Unlock()
	go func() {
		defer network.workers.Done()
		info, err := network.endpoint.Inspect(network.ctx, address)
		discovered := err == nil
		if err == nil && info.Execution {
			err = info.Compatibility()
		}
		network.mu.Lock()
		if network.closed || network.peers[id] != peer {
			network.mu.Unlock()
			return
		}
		if discovered {
			peer.info = info
		}
		create := err == nil && info.Execution && peer.controller == nil
		controller := peer.controller
		if create {
			controller = client.New(network.endpoint.Remote(address))
		}
		watch := discovered && peer.cancel == nil
		var ctx context.Context
		if watch {
			ctx, peer.cancel = context.WithCancel(network.ctx)
		}
		network.mu.Unlock()
		if create {
			err = controller.Start(network.ctx)
		} else if err == nil && controller != nil && !controller.Connected() {
			err = controller.Refresh(network.ctx)
		}
		network.mu.Lock()
		retained := !network.closed && network.peers[id] == peer
		if retained {
			peer.inspecting, peer.err, peer.online = false, err, err == nil
			if create && err == nil {
				peer.controller = controller
				peer.release = controller.OnChange(network.changed)
			}
		}
		network.mu.Unlock()
		if create && (!retained || err != nil) {
			_ = controller.Close()
		}
		if !retained {
			return
		}
		if watch {
			network.watchConnection(ctx, peer, address)
		}
		network.changed()
	}()
}

func (network *Network) watchConnection(ctx context.Context, peer *networkPeer, address link.EndpointAddress) {
	network.mu.Lock()
	if network.closed {
		network.mu.Unlock()
		return
	}
	network.workers.Add(1)
	network.mu.Unlock()
	go func() {
		defer network.workers.Done()
		_ = network.endpoint.Remote(address).WatchConnection(ctx, func(value link.ConnectionInfo) {
			network.mu.Lock()
			if network.peers[address.ID] == peer {
				peer.connection = value
				peer.online = value.Route != nil
			}
			network.mu.Unlock()
			network.changed()
		})
	}()
}

func (network *Network) closePeer(peer *networkPeer) {
	if peer.cancel != nil {
		peer.cancel()
	}
	if peer.release != nil {
		peer.release()
	}
	if peer.controller != nil {
		_ = peer.controller.Close()
	}
}

func (network *Network) Close() error {
	network.mu.Lock()
	if network.closed {
		network.mu.Unlock()
		return nil
	}
	network.closed = true
	network.cancel()
	peers := make([]*networkPeer, 0, len(network.peers))
	for _, peer := range network.peers {
		peers = append(peers, peer)
	}
	network.callbacks = map[uint64]func(){}
	network.mu.Unlock()
	network.workers.Wait()
	var result error
	for _, peer := range peers {
		if peer.release != nil {
			peer.release()
		}
		if peer.controller != nil {
			result = errors.Join(result, peer.controller.Close())
		}
	}
	return result
}
