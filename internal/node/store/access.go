package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// initializeResourceUsers preserves the legacy local audience without making
// previously private resources visible to newly paired users.
func initializeResourceUsers(ctx context.Context, db *sql.Conn) error {
	for _, table := range []string{"projects", "sessions", "worktrees", "terminals", "notifications"} {
		if _, err := db.ExecContext(ctx, "UPDATE "+table+" SET owner=(SELECT id FROM users WHERE local=1) WHERE owner IS NULL"); err != nil {
			return err
		}
	}
	_, err := db.ExecContext(ctx, "UPDATE turns SET user=(SELECT id FROM users WHERE local=1) WHERE user IS NULL")
	return err
}

func accessResult(ctx context.Context, db *sql.Conn, err error, allowed bool) error {
	if errors.Is(err, sql.ErrNoRows) {
		user, userErr := CurrentUser(ctx, db)
		if userErr != nil {
			return userErr
		}
		if user.Local {
			return nil
		}
	}
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	if !allowed {
		return fault(protocol.ErrorCodePermissionDenied, "resource access is unavailable")
	}
	return nil
}

func CheckProjectAccess(ctx context.Context, db *sql.Conn, id protocol.ProjectID, manage bool) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	var allowed bool
	err = db.QueryRowContext(ctx, `SELECT CASE WHEN ?3 THEN coalesce(owner,(SELECT id FROM users WHERE local=1))=?2 OR (?4 AND visibility='shared') ELSE visibility='shared' OR coalesce(owner,(SELECT id FROM users WHERE local=1))=?2 END FROM projects WHERE id=?1`, id.String(), user.ID.String(), manage, user.Administrator).Scan(&allowed)
	return accessResult(ctx, db, err, allowed)
}

const sessionAccessPredicate = `CASE WHEN s.project IS NULL THEN coalesce(s.owner,(SELECT id FROM users WHERE local=1))=?2 ELSE p.visibility='shared' OR coalesce(p.owner,(SELECT id FROM users WHERE local=1))=?2 END`

func CheckSessionAccess(ctx context.Context, db *sql.Conn, id protocol.SessionID) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	var allowed bool
	err = db.QueryRowContext(ctx, `SELECT `+sessionAccessPredicate+` FROM sessions s LEFT JOIN projects p ON p.id=s.project WHERE s.id=?1`, id.String(), user.ID.String()).Scan(&allowed)
	return accessResult(ctx, db, err, allowed)
}

func CheckWorktreeAccess(ctx context.Context, db *sql.Conn, id protocol.WorktreeID) error {
	return checkWorktreeAccess(ctx, db, id, false)
}

func checkWorktreeAccess(ctx context.Context, db *sql.Conn, id protocol.WorktreeID, requirePresent bool) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	query := `SELECT CASE WHEN w.project IS NULL THEN coalesce(w.owner,(SELECT id FROM users WHERE local=1))=?2 ELSE p.visibility='shared' OR coalesce(p.owner,(SELECT id FROM users WHERE local=1))=?2 END FROM worktrees w LEFT JOIN projects p ON p.id=w.project WHERE w.id=?1`
	if requirePresent {
		query += ` AND NOT EXISTS(SELECT 1 FROM removed_worktrees WHERE worktree=w.id)
		AND (w.project IS NULL OR p.id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM removed_projects WHERE project=p.id))`
	}
	var allowed bool
	err = db.QueryRowContext(ctx, query, id.String(), user.ID.String()).Scan(&allowed)
	if requirePresent && errors.Is(err, sql.ErrNoRows) {
		err = nil // An absent worktree cannot authorize a terminal subscription.
	}
	return accessResult(ctx, db, err, allowed)
}

func CheckTerminalAccess(ctx context.Context, db *sql.Conn, id protocol.TerminalID) error {
	worktree, owner, err := readTerminalScope(ctx, db, id)
	if err != nil {
		return accessResult(ctx, db, err, false)
	}
	return checkTerminalScope(ctx, db, worktree, owner, false)
}

// CheckTerminalSubscriptionAccess permits only an already attached stream to
// drain a finished terminal. The retained scope is not a retained grant.
func CheckTerminalSubscriptionAccess(ctx context.Context, db *sql.Conn, id protocol.TerminalID, attached bool) error {
	worktree, owner, err := readTerminalScope(ctx, db, id)
	if attached && errors.Is(err, sql.ErrNoRows) {
		err = db.QueryRowContext(ctx, `SELECT json_extract(audience,'$.terminal_worktree'),json_extract(audience,'$.owner')
		FROM removed_resource_audiences WHERE kind='terminal' AND id=?1
		AND json_type(audience,'$.terminal_worktree') IN ('null','text')`, id.String()).Scan(&worktree, &owner)
	}
	if errors.Is(err, sql.ErrNoRows) {
		return accessResult(ctx, db, nil, false)
	}
	if err != nil {
		return err
	}
	return checkTerminalScope(ctx, db, worktree, owner, true)
}

func readTerminalScope(ctx context.Context, db *sql.Conn, id protocol.TerminalID) (worktree, owner sql.NullString, err error) {
	err = db.QueryRowContext(ctx, "SELECT worktree,owner FROM terminals WHERE id=?1", id.String()).Scan(&worktree, &owner)
	return
}

func checkTerminalScope(ctx context.Context, db *sql.Conn, worktree, owner sql.NullString, requirePresent bool) error {
	if worktree.Valid {
		id, err := protocol.ParseWorktreeID(worktree.String)
		if err != nil {
			return err
		}
		return checkWorktreeAccess(ctx, db, id, requirePresent)
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	if !owner.Valid {
		local, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		owner.String = local.ID.String()
	}
	return accessResult(ctx, db, nil, owner.String == user.ID.String())
}

func CheckTurnAccess(ctx context.Context, db *sql.Conn, id protocol.TurnID) error {
	var session string
	if err := db.QueryRowContext(ctx, "SELECT session FROM turns WHERE id=?1", id.String()).Scan(&session); err != nil {
		return accessResult(ctx, db, err, false)
	}
	parsed, err := protocol.ParseSessionID(session)
	if err != nil {
		return err
	}
	return CheckSessionAccess(ctx, db, parsed)
}

func CheckArtifactAccess(ctx context.Context, db *sql.Conn, id protocol.ArtifactID) error {
	var session string
	if err := db.QueryRowContext(ctx, "SELECT session FROM artifacts WHERE id=?1", id.String()).Scan(&session); err != nil {
		return accessResult(ctx, db, err, false)
	}
	parsed, err := protocol.ParseSessionID(session)
	if err != nil {
		return err
	}
	return CheckSessionAccess(ctx, db, parsed)
}

func CheckNotificationAccess(ctx context.Context, db *sql.Conn, id protocol.NotificationID) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	var session, owner sql.NullString
	err = db.QueryRowContext(ctx, "SELECT json_extract(body,'$.content.session'),owner FROM notifications WHERE id=?1", id.String()).Scan(&session, &owner)
	if err != nil {
		return accessResult(ctx, db, err, false)
	}
	if session.Valid {
		id, err := protocol.ParseSessionID(session.String)
		if err != nil {
			return err
		}
		return CheckSessionAccess(ctx, db, id)
	}
	if !owner.Valid {
		local, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		owner.String = local.ID.String()
	}
	return accessResult(ctx, db, nil, owner.String == user.ID.String())
}

func captureWorktreeUser(ctx context.Context, db *sql.Conn, id protocol.WorktreeID, user protocol.UserID) error {
	_, err := db.ExecContext(ctx, "UPDATE worktrees SET owner=?2 WHERE id=?1 AND owner IS NULL", id.String(), user.String())
	return err
}

func CheckChildAccess(ctx context.Context, db *sql.Conn, kind, id string) error {
	var query string
	switch kind {
	case "approval":
		query = "SELECT turn FROM agent_approvals WHERE id=?1"
	case "question":
		query = "SELECT turn FROM agent_questions WHERE id=?1"
	case "checkpoint":
		query = "SELECT turn FROM file_checkpoints WHERE id=?1"
	case "attachment":
		var worktree string
		err := db.QueryRowContext(ctx, "SELECT worktree FROM attachments WHERE id=?1", id).Scan(&worktree)
		if err != nil {
			return accessResult(ctx, db, err, false)
		}
		parsed, err := protocol.ParseWorktreeID(worktree)
		if err != nil {
			return err
		}
		return CheckWorktreeAccess(ctx, db, parsed)
	default:
		return fault(protocol.ErrorCodePermissionDenied, "resource access is unavailable")
	}
	var turn string
	if err := db.QueryRowContext(ctx, query, id).Scan(&turn); err != nil {
		return accessResult(ctx, db, err, false)
	}
	parsed, err := protocol.ParseTurnID(turn)
	if err != nil {
		return err
	}
	return CheckTurnAccess(ctx, db, parsed)
}

func accessibleSessionOrder(ctx context.Context, db *sql.Conn, ids []protocol.SessionID) ([]protocol.SessionID, error) {
	values := make([]protocol.SessionID, 0, len(ids))
	for _, id := range ids {
		if err := CheckSessionAccess(ctx, db, id); err != nil {
			var denied *protocol.Fault
			if errors.As(err, &denied) && denied.Code == protocol.ErrorCodePermissionDenied {
				continue
			}
			return nil, err
		}
		values = append(values, id)
	}
	return values, nil
}

func ReadTerminals(ctx context.Context, db *sql.Conn) ([]protocol.TerminalInfo, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, err
	}
	return readBodies[protocol.TerminalInfo](ctx, db, `SELECT t.body FROM terminals t LEFT JOIN worktrees w ON w.id=t.worktree LEFT JOIN projects p ON p.id=w.project WHERE CASE WHEN t.worktree IS NULL THEN coalesce(t.owner,(SELECT id FROM users WHERE local=1))=?1 WHEN w.project IS NULL THEN coalesce(w.owner,(SELECT id FROM users WHERE local=1))=?1 ELSE p.visibility='shared' OR coalesce(p.owner,(SELECT id FROM users WHERE local=1))=?1 END ORDER BY t.id`, user.ID.String())
}
