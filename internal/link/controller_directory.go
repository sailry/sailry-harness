package link

import (
	"context"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (store *controllerStore) Relationships(ctx context.Context) ([]PeerRelationship, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	values := make([]PeerRelationship, 0, len(store.peers))
	for _, peer := range store.peers {
		values = append(values, PeerRelationship{ID: peer.ID, Direct: peer.Direct, MyDevice: peer.MyDevice, Removed: peer.Removed, Sources: append([]protocol.NodeID{}, peer.Sources...)})
	}
	return values, nil
}

func removeControllerSource(peers *[]controllerPeer, source protocol.NodeID) {
	for index := range *peers {
		peer := &(*peers)[index]
		peer.Sources = slices.DeleteFunc(peer.Sources, func(id protocol.NodeID) bool { return id == source })
	}
	*peers = slices.DeleteFunc(*peers, func(peer controllerPeer) bool { return !peer.Direct && !peer.Removed && len(peer.Sources) == 0 })
}

func (store *controllerStore) ApplyDirectory(ctx context.Context, source protocol.NodeID, addresses []EndpointAddress) error {
	return store.update(ctx, func(peers *[]controllerPeer) error {
		verified := false
		for _, peer := range *peers {
			if peer.ID == source && peer.MyDevice && !peer.Removed {
				verified = true
			}
		}
		if !verified {
			return pairingRejected()
		}
		// Remove just this source's origins. Other sources and direct pairings
		// continue to own the same deduplicated peer identity.
		for index := range *peers {
			(*peers)[index].Sources = slices.DeleteFunc((*peers)[index].Sources, func(id protocol.NodeID) bool { return id == source })
		}
		for _, address := range addresses {
			found := false
			for index := range *peers {
				peer := &(*peers)[index]
				if peer.ID != address.ID {
					continue
				}
				found = true
				if peer.Removed {
					break
				}
				peer.Sources = append(peer.Sources, source)
				copy := cloneAddress(address)
				if peer.Address != nil {
					copy = mergeAddress(*peer.Address, address)
				}
				peer.Address = &copy
				break
			}
			if !found {
				copy := cloneAddress(address)
				*peers = append(*peers, controllerPeer{ID: address.ID, Address: &copy, Direction: DirectionHost, Sources: []protocol.NodeID{source}})
			}
		}
		*peers = slices.DeleteFunc(*peers, func(peer controllerPeer) bool { return !peer.Direct && !peer.Removed && len(peer.Sources) == 0 })
		return nil
	})
}
