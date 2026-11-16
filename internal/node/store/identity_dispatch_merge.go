package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func mergeIdentityDispatch(ctx context.Context, db *sql.Conn, source, target protocol.User) error {
	// Evaluate queued handler conflicts before promoting any source handlers.
	// Running jobs retain their exact callback/request and immutable storage
	// owner. A conflicting queued callback stays archived and cannot execute
	// another user's handler merely because its name happens to match.
	statements := []string{
		`UPDATE dispatch_jobs SET user=?2 WHERE user=?1 AND (status<>'queued' OR package='' OR NOT EXISTS(
SELECT 1 FROM dispatch_user_handlers t WHERE t.user=?2 AND t.package=dispatch_jobs.package AND t.name=json_extract(dispatch_jobs.body,'$.handler')))`,
		`UPDATE dispatch_events SET user=?2 WHERE user=?1`,
		`INSERT INTO dispatch_user_handlers(user,package,name,source,topic,body,reference)
SELECT ?2,package,name,source,topic,body,reference FROM dispatch_user_handlers WHERE user=?1 AND package<>''
ON CONFLICT(user,package,name) DO NOTHING`,
		`INSERT INTO dispatch_user_schedules(user,package,id,next_ms,body)
SELECT ?2,package,id,next_ms,body FROM dispatch_user_schedules WHERE user=?1 AND package<>''
ON CONFLICT(user,package,id) DO NOTHING`,
	}
	for _, statement := range statements {
		if _, err := db.ExecContext(ctx, statement, source.ID.String(), target.ID.String()); err != nil {
			return err
		}
	}
	if source.Local && target.Local {
		for _, statement := range []string{
			`INSERT INTO dispatch_user_handlers(user,package,name,source,topic,body,reference) SELECT ?2,package,name,source,topic,body,reference FROM dispatch_user_handlers WHERE user=?1 AND package='' ON CONFLICT(user,package,name) DO NOTHING`,
			`INSERT INTO dispatch_user_schedules(user,package,id,next_ms,body) SELECT ?2,package,id,next_ms,body FROM dispatch_user_schedules WHERE user=?1 AND package='' ON CONFLICT(user,package,id) DO NOTHING`,
		} {
			if _, err := db.ExecContext(ctx, statement, source.ID.String(), target.ID.String()); err != nil {
				return err
			}
		}
		for _, statement := range []string{
			`DELETE FROM dispatch_user_handlers WHERE user=?1 AND package=''`,
			`DELETE FROM dispatch_user_schedules WHERE user=?1 AND package=''`,
		} {
			if _, err := db.ExecContext(ctx, statement, source.ID.String()); err != nil {
				return err
			}
		}
	}
	return nil
}
