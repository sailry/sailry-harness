package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// SessionLeftover is the worktree a removed session leaves without a user.
type SessionLeftover struct {
	Session  protocol.SessionID
	Worktree protocol.Worktree
}

// ReadSessionLeftovers reports, for each of sessions that is removed, the
// worktrees it used, now or in an earlier revision, that no remaining session
// uses or used. A main worktree and one whose removal is already recorded are
// never a leftover.
func ReadSessionLeftovers(ctx context.Context, db *sql.Conn, sessions []protocol.SessionID) ([]SessionLeftover, error) {
	values := []SessionLeftover{}
	for _, session := range sessions {
		rows, err := db.QueryContext(ctx, `SELECT w.id,w.project,w.path FROM worktrees w
 WHERE w.main=0 AND w.id NOT IN (SELECT worktree FROM removed_worktrees)
 AND EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND state='removed')
 AND (w.id=(SELECT worktree FROM sessions WHERE id=?1) OR w.id IN (SELECT worktree FROM session_revisions WHERE session=?1))
 AND NOT EXISTS(SELECT 1 FROM sessions o WHERE o.state!='removed' AND (o.worktree=w.id
 OR EXISTS(SELECT 1 FROM session_revisions r WHERE r.session=o.id AND r.worktree=w.id)))`, session.String())
		if err != nil {
			return nil, err
		}
		for rows.Next() {
			value := SessionLeftover{Session: session}
			var id string
			var project sql.NullString
			if err := rows.Scan(&id, &project, &value.Worktree.Path); err != nil {
				rows.Close()
				return nil, err
			}
			if value.Worktree.ID, err = protocol.ParseWorktreeID(id); err != nil {
				rows.Close()
				return nil, err
			}
			if project.Valid {
				owner, err := protocol.ParseProjectID(project.String)
				if err != nil {
					rows.Close()
					return nil, err
				}
				value.Worktree.Project = &owner
			}
			values = append(values, value)
		}
		if err := errors.Join(rows.Err(), rows.Close()); err != nil {
			return nil, err
		}
	}
	return values, nil
}
