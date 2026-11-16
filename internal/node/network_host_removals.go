package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (peers nodePeerStore) PrepareHostRemoval(ctx context.Context, host protocol.NodeID, request protocol.Request) (link.HostRemovalAttempt, error) {
	var value link.HostRemovalAttempt
	err := peers.handle.Write(ctx, func(db *sql.Conn) error {
		var err error
		value, err = store.PrepareHostRemoval(ctx, db, host, request)
		return err
	})
	return value, nodeFault(err)
}
func (peers nodePeerStore) RecordHostRemoval(ctx context.Context, host protocol.NodeID, request protocol.RequestID, state string, failure *protocol.Fault) error {
	return nodeFault(peers.handle.Write(ctx, func(db *sql.Conn) error { return store.RecordHostRemoval(ctx, db, host, request, state, failure) }))
}
func (peers nodePeerStore) HostRemovals(ctx context.Context) ([]link.HostRemovalAttempt, error) {
	var values []link.HostRemovalAttempt
	err := peers.handle.Read(ctx, func(db *sql.Conn) error { var err error; values, err = store.HostRemovals(ctx, db); return err })
	return values, nodeFault(err)
}
