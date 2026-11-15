package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func initializeDispatchUsers(ctx context.Context, db *sql.Conn) error {
	user, err := LocalUser(ctx, db)
	if err != nil {
		return err
	}
	for _, statement := range []string{
		"INSERT INTO dispatch_user_handlers(user,package,name,source,topic,body,reference) SELECT ?1,package,name,source,topic,body,reference FROM dispatch_handlers WHERE true ON CONFLICT(user,package,name) DO NOTHING",
		"INSERT INTO dispatch_user_schedules(user,package,id,next_ms,body) SELECT ?1,package,id,next_ms,body FROM dispatch_schedules WHERE true ON CONFLICT(user,package,id) DO NOTHING",
		"UPDATE dispatch_jobs SET user=?1 WHERE user IS NULL",
		"UPDATE dispatch_jobs SET storage_user=?1 WHERE storage_user IS NULL",
		"UPDATE dispatch_events SET user=?1 WHERE user IS NULL",
		"DELETE FROM dispatch_handlers", "DELETE FROM dispatch_schedules",
	} {
		if _, err := db.ExecContext(ctx, statement, user.ID.String()); err != nil {
			return err
		}
	}
	return nil
}

// DispatchJobContext restores a trusted scheduled operation's canonical actor
// separately from the immutable settings principal retained at enqueue time.
func DispatchJobContext(ctx context.Context, db *sql.Conn, identity protocol.JobID) (context.Context, error) {
	var owner, storage string
	if err := db.QueryRowContext(ctx, "SELECT user,coalesce(storage_user,user) FROM dispatch_jobs WHERE id=?1", identity.String()).Scan(&owner, &storage); err != nil {
		return nil, err
	}
	userID, err := protocol.ParseUserID(owner)
	if err != nil {
		return nil, err
	}
	user, err := ResolveUser(ctx, db, userID)
	if err != nil {
		return nil, err
	}
	captured, err := protocol.ParseUserID(storage)
	if err != nil {
		return nil, err
	}
	return WithCapturedUser(WithUser(ctx, user), captured), nil
}
