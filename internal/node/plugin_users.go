package node

import (
	"context"
	"database/sql"
	"path"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) pluginRuntimeUser(ctx context.Context) (protocol.UserID, error) {
	var user protocol.UserID
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		user, err = store.CapturedUser(ctx, db)
		return err
	})
	return user, err
}

func pluginProgramOwner(user protocol.UserID, name string) string {
	return user.String() + "\x00" + name
}

// Preserve the original local package directory while new users get their own
// writable package roots. Immutable package content remains digest-shared.
func (node *Node) pluginDataDirectory(ctx context.Context, name string) (string, error) {
	var directory string
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		user, err := store.CapturedUser(ctx, db)
		if err != nil {
			return err
		}
		local, err := store.LegacyStorageUser(ctx, db)
		if err != nil {
			return err
		}
		if user == local {
			directory = path.Join("plugins", "data", name)
		} else {
			directory = path.Join("plugins", "users", user.String(), "data", name)
		}
		return nil
	})
	return directory, err
}

func (node *Node) pluginOwnerContext(ctx context.Context) (context.Context, error) {
	var user protocol.User
	var storage protocol.UserID
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		user, err = store.CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		storage, err = store.CapturedUser(ctx, db)
		return err
	})
	if err != nil {
		return nil, err
	}
	return store.WithCapturedUser(store.WithUser(node.ctx, user), storage), nil
}
