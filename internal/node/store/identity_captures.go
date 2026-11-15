package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type capturedUserKey struct{}

// InitializeIdentity fixes the owner of legacy unprefixed storage namespaces.
// Its identity stays stable when the local administrator flag moves in a merge.
func InitializeIdentity(ctx context.Context, db *sql.Conn) error {
	_, err := db.ExecContext(ctx, "INSERT INTO identity_storage_origin(singleton,user) SELECT 1,id FROM users WHERE local=1 ON CONFLICT(singleton) DO NOTHING")
	return err
}

func LegacyStorageUser(ctx context.Context, db *sql.Conn) (protocol.UserID, error) {
	var id string
	if err := db.QueryRowContext(ctx, "SELECT coalesce((SELECT user FROM identity_storage_origin WHERE singleton=1),(SELECT id FROM users WHERE local=1))").Scan(&id); err != nil {
		return protocol.UserID{}, err
	}
	return protocol.ParseUserID(id)
}

// WithCapturedUser retains the immutable storage principal of admitted work.
// CurrentUser remains the canonical actor for authorization after a user merge.
func WithCapturedUser(ctx context.Context, user protocol.UserID) context.Context {
	return context.WithValue(ctx, capturedUserKey{}, user)
}

func CapturedUser(ctx context.Context, db *sql.Conn) (protocol.UserID, error) {
	if user, ok := ctx.Value(capturedUserKey{}).(protocol.UserID); ok {
		return user, nil
	}
	user, err := CurrentUser(ctx, db)
	return user.ID, err
}

// OriginalTurnUser does not follow merged_into: immutable settings revisions
// with the same number can contain different credentials for different users.
func OriginalTurnUser(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (protocol.User, error) {
	return readUser(ctx, db, `SELECT u.id,u.name,u.administrator,u.local,u.state,u.merged_into
FROM turns t JOIN users u ON u.id=coalesce((SELECT user FROM identity_turn_origins WHERE turn=t.id),t.user,(SELECT id FROM users WHERE local=1))
WHERE t.id=?1`, turn.String())
}

// ResolveUser follows only stored merge relationships; request bodies cannot
// supply a principal or select another user's resource namespace.
func ResolveUser(ctx context.Context, db *sql.Conn, id protocol.UserID) (protocol.User, error) {
	seen := map[protocol.UserID]bool{}
	for {
		if seen[id] {
			return protocol.User{}, invalid("invalid user merge cycle")
		}
		seen[id] = true
		user, err := readUser(ctx, db, "SELECT id,name,administrator,local,state,merged_into FROM users WHERE id=?1", id.String())
		if err != nil {
			return protocol.User{}, err
		}
		if user.State == "active" {
			return user, nil
		}
		if user.State != "merged" || user.MergedInto == nil {
			return protocol.User{}, invalid("user is unavailable")
		}
		id = *user.MergedInto
	}
}
