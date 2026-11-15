package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (peers nodePeerStore) NarrowLegacyController(ctx context.Context, peer protocol.NodeID) error {
	return nodeFault(peers.handle.Write(ctx, func(db *sql.Conn) error { return store.NarrowLegacyController(ctx, db, peer) }))
}
