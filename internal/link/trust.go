package link

import (
	"context"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// PeerDirection describes which endpoint may call the other's execution service.
type PeerDirection string

const (
	DirectionController PeerDirection = "controller"
	DirectionHost       PeerDirection = "host"
	DirectionBoth       PeerDirection = "both"
)

func (direction PeerDirection) Valid() bool {
	return direction == DirectionController || direction == DirectionHost || direction == DirectionBoth
}

func (direction PeerDirection) AllowsInbound() bool {
	return direction == DirectionController || direction == DirectionBoth
}

func (direction PeerDirection) AllowsOutbound() bool {
	return direction == DirectionHost || direction == DirectionBoth
}

// PeerTrust is one persisted relationship. Both is retained for legacy pairings
// or two independently accepted invitations in opposite directions.
type PeerTrust struct {
	ID        protocol.NodeID `json:"id"`
	Direction PeerDirection   `json:"direction"`
	Legacy    bool            `json:"legacy,omitempty"`
}

func mergeDirection(current, added PeerDirection) PeerDirection {
	if current == "" || current == added {
		return added
	}
	return DirectionBoth
}

func (link *Link) Direction(peer protocol.NodeID) PeerDirection {
	link.mu.RLock()
	defer link.mu.RUnlock()
	if !link.trusted[peer] {
		return ""
	}
	return link.directions[peer]
}

func (link *Link) PeerDirections(ctx context.Context) ([]PeerTrust, error) {
	return link.store.Directions(ctx)
}

// NarrowLegacyController runs only after authenticated inspection positively
// identifies a controller without execution. The store checks legacy state in
// the same write, so a newer explicit pairing cannot be narrowed by a stale read.
func (link *Link) NarrowLegacyController(ctx context.Context, peer protocol.NodeID) error {
	store, ok := link.store.(interface {
		NarrowLegacyController(context.Context, protocol.NodeID) error
	})
	if !ok {
		return nil
	}
	return link.updateTrust(ctx, peer, func() error { return store.NarrowLegacyController(ctx, peer) })
}

// SetDirection changes an existing relationship and releases connections that
// held access removed by the change. It does not pair an unknown peer.
func (link *Link) SetDirection(ctx context.Context, peer protocol.NodeID, direction PeerDirection) error {
	if !direction.Valid() {
		return invalidLink("invalid peer direction")
	}
	return link.updateTrust(ctx, peer, func() error {
		return link.store.SetDirection(ctx, peer, direction)
	})
}

func (link *Link) pairPeer(ctx context.Context, peer protocol.NodeID, direction PeerDirection) error {
	return link.updateTrust(ctx, peer, func() error {
		link.cancelHostRemoval(peer)
		return link.store.PairPeer(ctx, peer, direction)
	})
}

func (link *Link) updateTrust(ctx context.Context, peer protocol.NodeID, update func() error) error {
	link.trustUpdate.Lock()
	defer link.trustUpdate.Unlock()
	if err := link.ctx.Err(); err != nil {
		return linkNetwork(err)
	}
	if err := update(); err != nil {
		if faultValue(err).Code == protocol.ErrorCodeOutcomeUnknown {
			reconciled := link.reconcilePeer(ctx, peer)
			if reconciled != nil {
				link.applyTrust(peer, "")
			}
			return errors.Join(err, reconciled)
		}
		return err
	}
	if err := link.reconcilePeer(ctx, peer); err != nil {
		link.applyTrust(peer, "")
		return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "peer access was saved but its live state could not be confirmed"}
	}
	return nil
}
