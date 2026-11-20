package node

import (
	"context"
	"database/sql"
	"log/slog"
	"os"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type nodePeerStore struct {
	handle *store.Handle
	node   protocol.NodeID
	owner  *Node
}

func (peers nodePeerStore) Peers(ctx context.Context) ([]protocol.NodeID, error) {
	var values []protocol.NodeID
	err := peers.handle.Read(ctx, func(db *sql.Conn) error {
		var err error
		values, err = store.LinkPeers(ctx, db)
		return err
	})
	return values, nodeFault(err)
}

func (peers nodePeerStore) Addresses(ctx context.Context) ([]link.EndpointAddress, error) {
	var values []link.EndpointAddress
	err := peers.handle.Read(ctx, func(db *sql.Conn) error {
		var err error
		values, err = store.LinkAddresses(ctx, db)
		return err
	})
	return values, nodeFault(err)
}

func (peers nodePeerStore) Remember(ctx context.Context, address link.EndpointAddress) error {
	return nodeFault(peers.handle.Write(ctx, func(db *sql.Conn) error {
		return store.RememberLinkPeer(ctx, db, address)
	}))
}

func (peers nodePeerStore) SetTrust(ctx context.Context, peer protocol.NodeID, trusted bool) error {
	return nodeFault(peers.handle.Write(ctx, func(db *sql.Conn) error {
		if !trusted {
			return store.RevokeIdentityDevice(ctx, db, peers.node, peers.node, protocol.RevokeDevice{Device: peer})
		}
		return store.SetLinkTrust(ctx, db, peer, trusted)
	}))
}

func (peers nodePeerStore) Directions(ctx context.Context) ([]link.PeerTrust, error) {
	var values []link.PeerTrust
	err := peers.handle.Read(ctx, func(db *sql.Conn) error { var err error; values, err = store.LinkDirections(ctx, db); return err })
	return values, nodeFault(err)
}
func (peers nodePeerStore) PairPeer(ctx context.Context, peer protocol.NodeID, direction link.PeerDirection) error {
	return nodeFault(peers.handle.Write(ctx, func(db *sql.Conn) error { return store.PairLinkPeer(ctx, db, peer, direction) }))
}
func (peers nodePeerStore) SetDirection(ctx context.Context, peer protocol.NodeID, direction link.PeerDirection) error {
	return nodeFault(peers.handle.Write(ctx, func(db *sql.Conn) error { return store.SetIdentityPeerDirection(ctx, db, peer, direction) }))
}

func (peers nodePeerStore) SavePairing(ctx context.Context, address link.EndpointAddress, direction link.PeerDirection, scope link.InvitationScope, hasNode *bool, resume bool) error {
	var user protocol.User
	initialize := peers.owner != nil && direction.AllowsInbound() && (hasNode == nil || *hasNode)
	err := peers.handle.Write(ctx, func(db *sql.Conn) error {
		var err error
		if resume {
			// Device revocation can commit before Link refreshes its live trust.
			// Check the persisted inbound user in this same address write.
			user, err = store.CallerUser(ctx, db, peers.node, address.ID)
			if err != nil {
				return err
			}
			return store.RememberLinkPeer(ctx, db, address)
		}
		if hasNode == nil {
			err = store.PairIdentityPeer(ctx, db, peers.node, address.ID, direction, scope)
		} else {
			err = store.PairPeerIdentity(ctx, db, peers.node, address.ID, direction, *hasNode)
		}
		if err != nil {
			return err
		}
		if err = store.RememberLinkPeer(ctx, db, address); err != nil || !initialize {
			return err
		}
		user, err = store.CallerUser(ctx, db, peers.node, address.ID)
		return err
	})
	if err != nil {
		return nodeFault(err)
	}
	if initialize {
		if err := peers.owner.InitializeUserPlugins(ctx, user); err != nil {
			return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the peer was saved but its defaults were not initialized; retry the same pairing"}
		}
	}
	return nil
}

func (peers nodePeerStore) ClassifyPeer(ctx context.Context, peer protocol.NodeID, hasNode bool) error {
	err := peers.handle.Write(ctx, func(db *sql.Conn) error {
		return store.ClassifyPeer(ctx, db, peers.node, peer, hasNode)
	})
	if err != nil || !hasNode || peers.owner == nil {
		return nodeFault(err)
	}
	var user protocol.User
	inbound := false
	err = peers.handle.Read(ctx, func(db *sql.Conn) error {
		directions, err := store.LinkDirections(ctx, db)
		if err != nil {
			return err
		}
		for _, value := range directions {
			if value.ID == peer {
				inbound = value.Direction.AllowsInbound()
				break
			}
		}
		if !inbound {
			return nil
		}
		user, err = store.CallerUser(ctx, db, peers.node, peer)
		return err
	})
	if err == nil && inbound {
		err = peers.owner.InitializeUserPlugins(ctx, user)
	}
	return nodeFault(err)
}

func (peers nodePeerStore) Relationships(ctx context.Context) ([]link.PeerRelationship, error) {
	var values []link.PeerRelationship
	err := peers.handle.Read(ctx, func(db *sql.Conn) error {
		var err error
		values, err = store.IdentityPeerRelationships(ctx, db)
		return err
	})
	return values, nodeFault(err)
}

func (peers nodePeerStore) ApplyDirectory(ctx context.Context, source protocol.NodeID, revision uint64, addresses []link.EndpointAddress, names ...map[protocol.NodeID]string) error {
	return nodeFault(peers.handle.Write(ctx, func(db *sql.Conn) error {
		return store.ApplyIdentityDirectory(ctx, db, source, revision, addresses, names...)
	}))
}

func (peers nodePeerStore) RememberPeerName(ctx context.Context, peer protocol.NodeID, name string) error {
	return nodeFault(peers.handle.Write(ctx, func(db *sql.Conn) error { return store.RememberIdentityPeerName(ctx, db, peer, name) }))
}

func (node *Node) Link() *link.Link { return node.network }

func (node *Node) bindNetwork(ctx context.Context, scope link.NetworkScope) error {
	if scope.Internet || len(scope.Relays) != 0 {
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			_, selected, err := store.ReadNetworkSelection(ctx, db)
			if err != nil || selected == nil {
				return err
			}
			if selected.Kind == "direct" {
				scope.Internet, scope.Relays = true, nil
				return nil
			}
			relays, err := link.NetworkRelayURLs(ctx, *selected)
			if err != nil {
				slog.Warn("Saved relay selection is unavailable; using startup network")
				return nil
			}
			scope.Internet, scope.Relays = false, relays
			return nil
		})
		if err != nil {
			return err
		}
	}
	name, err := os.Hostname()
	if err != nil {
		name = ""
	}
	node.network, err = link.Bind(node.ctx, link.EndpointOptions{
		Identity: node.profile.Identity,
		Scope:    scope, Handler: node,
		Peers: nodePeerStore{handle: node.profile.Store.Node, node: node.ID(), owner: node}, Name: name,
		Paired: node.reauthenticateResourceHost,
	})
	if err == nil {
		err = node.startResourceObservers()
	}
	return err
}
