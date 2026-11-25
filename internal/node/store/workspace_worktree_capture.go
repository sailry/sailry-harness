package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ProjectWorktree contains only durable resource identity, not filesystem observations.
type ProjectWorktree struct {
	Worktree protocol.Worktree
	Project  protocol.Project
}

func ReadProjectWorktree(ctx context.Context, db *sql.Conn, id protocol.WorktreeID) (ProjectWorktree, error) {
	result := ProjectWorktree{}
	var projectID string
	var appearance []byte
	err := db.QueryRowContext(ctx, "SELECT w.id,w.project,w.path,w.main,p.name,p.path,p.appearance FROM worktrees w JOIN projects p ON p.id=w.project WHERE w.id=?1 AND w.id NOT IN (SELECT worktree FROM removed_worktrees) AND p.id NOT IN (SELECT project FROM removed_projects)", id.String()).Scan(new(string), &projectID, &result.Worktree.Path, &result.Worktree.Main, &result.Project.Name, &result.Project.Path, &appearance)
	if errors.Is(err, sql.ErrNoRows) {
		return result, fault(protocol.ErrorCodeNotFound, "worktree does not exist on this Node")
	}
	if err != nil {
		return result, err
	}
	result.Worktree.ID = id
	result.Project.ID, err = protocol.ParseProjectID(projectID)
	if err != nil {
		return result, err
	}
	result.Worktree.Project = &result.Project.ID
	if err := decodeWorkspaceAppearance(appearance, &result.Project.Appearance); err != nil {
		return result, err
	}
	return result, nil
}
func PrepareWorkspaceWorktreeRemoval(ctx context.Context, db *sql.Conn, id protocol.WorktreeID) (ProjectWorktree, error) {
	value, err := ReadProjectWorktree(ctx, db, id)
	if err != nil {
		return value, err
	}
	if value.Worktree.Main {
		return value, fault(protocol.ErrorCodeConflict, "cannot remove the main worktree")
	}
	var busy bool
	err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE r.worktree=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping')) OR EXISTS(SELECT 1 FROM terminals WHERE worktree=?1 AND json_extract(body,'$.status')!='closed')", id.String()).Scan(&busy)
	if err != nil {
		return value, err
	}
	if busy {
		return value, fault(protocol.ErrorCodeBusy, "worktree has active or queued resources")
	}
	return value, nil
}
