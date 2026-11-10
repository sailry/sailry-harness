package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func decodeWorkspaceAppearance(body []byte, value *protocol.ProjectsAppearance) error {
	return json.Unmarshal(body, value)
}

// RegisterCreatedWorktree commits a verified external checkout. No filesystem
// action or native Git command runs on the storage writer.
func RegisterCreatedWorktree(ctx context.Context, db *sql.Conn, value protocol.Worktree, adopt bool) (protocol.Worktree, []protocol.Event, error) {
	if value.Project == nil {
		return value, nil, invalid("managed worktree requires a project")
	}
	var present bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND id NOT IN (SELECT project FROM removed_projects))", value.Project.String()).Scan(&present); err != nil {
		return value, nil, err
	}
	if !present {
		return value, nil, fault(protocol.ErrorCodeNotFound, "project does not exist on this Node")
	}
	var id string
	var project sql.NullString
	var main, removed bool
	err := db.QueryRowContext(ctx, "SELECT id,project,main,EXISTS(SELECT 1 FROM removed_worktrees WHERE worktree=w.id) FROM worktrees w WHERE path=?1", value.Path).Scan(&id, &project, &main, &removed)
	if err == nil {
		if !project.Valid || project.String != value.Project.String() {
			return value, nil, fault(protocol.ErrorCodeConflict, "worktree is registered to another project")
		}
		if !removed && !adopt {
			return value, nil, fault(protocol.ErrorCodeConflict, "worktree directory is already registered")
		}
		value.ID, err = protocol.ParseWorktreeID(id)
		if err != nil {
			return value, nil, err
		}
		value.Main = main
		if !removed {
			return value, nil, nil
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM removed_worktrees WHERE worktree=?1", id); err != nil {
			return value, nil, err
		}
		value.Main = false
		return value, []protocol.Event{{Kind: "worktree_registered", Data: value}}, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return value, nil, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO worktrees(id,project,path,main) VALUES(?1,?2,?3,?4)", value.ID.String(), value.Project.String(), value.Path, value.Main); err != nil {
		return value, nil, err
	}
	return value, []protocol.Event{{Kind: "worktree_registered", Data: value}}, nil
}
func FinishRemovedWorktree(ctx context.Context, db *sql.Conn, id protocol.WorktreeID) ([]protocol.Event, error) {
	var referenced bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM sessions WHERE worktree=?1) OR EXISTS(SELECT 1 FROM session_revisions WHERE worktree=?1) OR EXISTS(SELECT 1 FROM agent_approval_rules WHERE worktree=?1)", id.String()).Scan(&referenced); err != nil {
		return nil, err
	}
	var result sql.Result
	var err error
	if referenced {
		for _, table := range []string{"terminals", "hook_settings"} {
			if _, err := db.ExecContext(ctx, "DELETE FROM "+table+" WHERE worktree=?1", id.String()); err != nil {
				return nil, err
			}
		}
		result, err = db.ExecContext(ctx, "INSERT INTO removed_worktrees(worktree) SELECT id FROM worktrees WHERE id=?1 AND main=0 AND id NOT IN (SELECT worktree FROM removed_worktrees)", id.String())
	} else {
		result, err = db.ExecContext(ctx, "DELETE FROM worktrees WHERE id=?1 AND main=0", id.String())
	}
	if err != nil {
		return nil, err
	}
	count, err := result.RowsAffected()
	if err != nil {
		return nil, err
	}
	if count != 1 {
		return nil, fault(protocol.ErrorCodeConflict, "worktree registration changed during removal")
	}
	return []protocol.Event{{Kind: "worktree_removed", Data: protocol.EventWorktreeRemovedPayload{ID: id}}}, nil
}

// RegisterClonedProject reuses the existing source registration after the native
// clone and canonical-directory validation have completed outside SQL.
func RegisterClonedProject(ctx context.Context, db *sql.Conn, draft protocol.ProjectsDraft, canonical string) (protocol.Output, []protocol.Event, error) {
	if err := validateProject(draft.Name, canonical, draft.Appearance); err != nil {
		return protocol.Output{}, nil, err
	}
	return createProject(ctx, db, draft, PreparedPaths{Path: canonical})
}
