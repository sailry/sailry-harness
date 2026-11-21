package node

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// RunCacheToolsAcceptance is an in-process Host acceptance entry point. The
// Node retains the captured credentials and returns only the measurements.
func (node *Node) RunCacheToolsAcceptance(ctx context.Context, config protocol.SessionConfig) ([]model.CacheToolsSample, error) {
	if err := node.begin(); err != nil {
		return nil, errors.New("The acceptance Node is unavailable")
	}
	defer node.work.Done()
	var binding model.Binding
	var captureErr error
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		binding, captureErr = store.CaptureCacheToolsBinding(ctx, db, node.ID(), config)
		return captureErr
	}); err != nil {
		if captureErr != nil {
			return nil, captureErr
		}
		return nil, errors.New("Could not read the cache-tools configuration")
	}
	return model.RunCacheToolsAcceptance(ctx, binding)
}
