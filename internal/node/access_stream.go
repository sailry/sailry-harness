package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type authorizedStream struct {
	*accessObservation
	source link.Stream
}

func (node *Node) Open(ctx context.Context, caller protocol.NodeID, id protocol.StreamID) (link.Stream, error) {
	verified, err := node.authenticate(ctx, caller)
	if err != nil {
		return nil, err
	}
	user := userFromContext(verified)
	scope, found, err := node.transfers.Scope(caller, user.ID, id)
	if err != nil {
		return nil, err
	}
	check := func(next context.Context) error {
		current, err := node.authenticate(store.WithUser(next, user), caller)
		if err != nil {
			return err
		}
		if scope.Draft && userFromContext(current).ID != user.ID {
			return permissionDenied()
		}
		if !found || scope.Worktree == (protocol.WorktreeID{}) {
			return nil
		}
		return node.profile.Store.Node.Read(current, func(db *sql.Conn) error {
			var present bool
			if err := db.QueryRowContext(current, "SELECT EXISTS(SELECT 1 FROM worktrees WHERE id=?1)", scope.Worktree.String()).Scan(&present); err != nil {
				return err
			}
			if !present {
				return permissionDenied()
			}
			return store.CheckWorktreeAccess(current, db, scope.Worktree)
		})
	}
	if err = check(ctx); err != nil {
		return nil, nodeFault(err)
	}
	stream, err := node.open(verified, caller, id)
	if err != nil {
		return nil, err
	}
	value := &authorizedStream{source: stream}
	value.accessObservation = node.observeAccess(check, stream.Close)
	go func() {
		for {
			select {
			case <-value.closed:
				return
			case <-node.closed:
				_ = value.Close()
				return
			case <-value.wake:
				if err := check(node.ctx); err != nil {
					_ = value.Close()
					return
				}
			}
		}
	}()
	return value, nil
}
func (value *authorizedStream) Read(data []byte) (int, error) {
	if err := value.check(value.node.ctx); err != nil {
		_ = value.Close()
		return 0, err
	}
	return value.source.Read(data)
}
func (value *authorizedStream) Write(data []byte) (int, error) {
	if err := value.check(value.node.ctx); err != nil {
		_ = value.Close()
		return 0, err
	}
	return value.source.Write(data)
}
func (value *authorizedStream) CloseWrite() error { return value.source.CloseWrite() }
