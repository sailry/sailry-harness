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

func accessResult(err error, allowed bool) error {
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
	return accessResult(err, allowed)
}

func CheckSessionAccess(ctx context.Context, db *sql.Conn, id protocol.SessionID) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	var allowed bool
	err = db.QueryRowContext(ctx, `SELECT CASE WHEN s.project IS NULL THEN coalesce(s.owner,(SELECT id FROM users WHERE local=1))=?2 ELSE p.visibility='shared' OR coalesce(p.owner,(SELECT id FROM users WHERE local=1))=?2 END FROM sessions s LEFT JOIN projects p ON p.id=s.project WHERE s.id=?1`, id.String(), user.ID.String()).Scan(&allowed)
	return accessResult(err, allowed)
}

func CheckWorktreeAccess(ctx context.Context, db *sql.Conn, id protocol.WorktreeID) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	var allowed bool
	err = db.QueryRowContext(ctx, `SELECT CASE WHEN w.project IS NULL THEN coalesce(w.owner,(SELECT id FROM users WHERE local=1))=?2 ELSE p.visibility='shared' OR coalesce(p.owner,(SELECT id FROM users WHERE local=1))=?2 END FROM worktrees w LEFT JOIN projects p ON p.id=w.project WHERE w.id=?1`, id.String(), user.ID.String()).Scan(&allowed)
	return accessResult(err, allowed)
}

func CheckTerminalAccess(ctx context.Context, db *sql.Conn, id protocol.TerminalID) error {
	var worktree sql.NullString
	var owner sql.NullString
	err := db.QueryRowContext(ctx, "SELECT worktree,owner FROM terminals WHERE id=?1", id.String()).Scan(&worktree, &owner)
	if err != nil {
		return accessResult(err, false)
	}
	if worktree.Valid {
		worktree, err := protocol.ParseWorktreeID(worktree.String)
		if err != nil {
			return err
		}
		return CheckWorktreeAccess(ctx, db, worktree)
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
	return accessResult(nil, owner.String == user.ID.String())
}

func CheckTurnAccess(ctx context.Context, db *sql.Conn, id protocol.TurnID) error {
	var session string
	if err := db.QueryRowContext(ctx, "SELECT session FROM turns WHERE id=?1", id.String()).Scan(&session); err != nil {
		return accessResult(err, false)
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
		return accessResult(err, false)
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
		return accessResult(err, false)
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
	return accessResult(nil, owner.String == user.ID.String())
}

// TurnUser reads the admitted principal even after the submitting device leaves.
func TurnUser(ctx context.Context, db *sql.Conn, id protocol.TurnID) (protocol.User, error) {
	return readUser(ctx, db, `SELECT u.id,u.name,u.administrator,u.local,u.state,u.merged_into FROM turns t JOIN users u ON u.id=coalesce(t.user,(SELECT id FROM users WHERE local=1)) WHERE t.id=?1`, id.String())
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
			return accessResult(err, false)
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
		return accessResult(err, false)
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
