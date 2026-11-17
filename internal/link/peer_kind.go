package link

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// PeerIdentityStore binds the type reported by an authenticated peer to its
// existing membership. Classification cannot add trust or execution direction.
type PeerIdentityStore interface {
	ClassifyPeer(context.Context, protocol.NodeID, bool) error
}

type peerIdentityPairStore interface {
	PairIdentity(context.Context, protocol.NodeID, PeerDirection, bool) error
}

func (link *Link) pairIdentity(ctx context.Context, peer protocol.NodeID, direction PeerDirection, hasNode bool) error {
	if store, ok := link.store.(peerIdentityPairStore); ok {
		if err := link.updateTrust(ctx, peer, func() error {
			link.cancelHostRemoval(peer)
			return store.PairIdentity(ctx, peer, direction, hasNode)
		}); err != nil {
			return err
		}
	} else if err := link.pairMember(ctx, peer, direction, InvitationScope{Kind: protocol.InvitationOtherUser}); err != nil {
		return err
	}
	return link.classifyPeer(ctx, peer, hasNode)
}

func (link *Link) classifyPeer(ctx context.Context, peer protocol.NodeID, hasNode bool) error {
	if !link.isTrusted(peer) {
		return pairingRejected()
	}
	if known, exists := link.peerKinds.Load(peer); exists {
		if known.(bool) != hasNode {
			return invalidLink("paired device type changed")
		}
		return nil
	}
	if store, ok := link.store.(PeerIdentityStore); ok {
		if err := store.ClassifyPeer(ctx, peer, hasNode); err != nil {
			return err
		}
	}
	known, loaded := link.peerKinds.LoadOrStore(peer, hasNode)
	if loaded && known.(bool) != hasNode {
		return invalidLink("paired device type changed")
	}
	return nil
}

func (link *Link) ownShell(ctx context.Context, peer protocol.NodeID) bool {
	kind, known := link.peerKinds.Load(peer)
	return known && !kind.(bool) && link.Direction(peer).AllowsInbound()
}

func (link *Link) directHost(ctx context.Context, peer protocol.NodeID) bool {
	return link.directRole(ctx, peer, true)
}

func (link *Link) directController(ctx context.Context, peer protocol.NodeID) bool {
	return link.directRole(ctx, peer, false)
}

func (link *Link) directRole(ctx context.Context, peer protocol.NodeID, outbound bool) bool {
	direction := link.Direction(peer)
	if outbound && !direction.AllowsOutbound() || !outbound && !direction.AllowsInbound() {
		return false
	}
	relationships, err := link.Relationships(ctx)
	if err != nil {
		return false
	}
	for _, relationship := range relationships {
		if relationship.ID == peer {
			return relationship.Direct && !relationship.Removed &&
				(outbound && relationship.DirectDirection.AllowsOutbound() || !outbound && relationship.DirectDirection.AllowsInbound())
		}
	}
	return false
}
