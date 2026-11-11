package node

import (
	"context"
	"database/sql"
	"os"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type nodePeerStore struct {
	handle *store.Handle
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
		return store.SetLinkTrust(ctx, db, peer, trusted)
	}))
}

func StartWithNetwork(ctx context.Context, path string, scope link.NetworkScope) (*Node, error) {
	return StartWithOptions(ctx, path, Options{Network: &scope})
}

func (node *Node) Link() *link.Link { return node.network }

func (node *Node) bindNetwork(ctx context.Context, scope link.NetworkScope) error {
	if err := iroh.LoadDefault(!core.Release()); err != nil {
		return err
	}
	if scope.Internet || len(scope.Relays) != 0 {
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			_, selected, err := store.ReadNetworkSelection(ctx, db)
			if err != nil || selected == nil {
				return err
			}
			scope.Internet = selected.Kind == "iroh"
			scope.Relays = nil
			if selected.Kind == "custom" {
				scope.Relays = append([]string{}, selected.Data.(*protocol.NetworkSelectionCustomPayload).Relays...)
			}
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
		Peers: nodePeerStore{handle: node.profile.Store.Node}, Name: name,
	})
	return err
}
