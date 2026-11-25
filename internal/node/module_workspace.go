package node

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type moduleWorkspaceScope struct {
	node    *Node
	ctx     context.Context
	root    string
	caller  protocol.NodeID
	request protocol.Request
	effect  *workspaceCompletion
}

func (s moduleWorkspaceScope) Root() string { return s.root }

func (s moduleWorkspaceScope) Changed() { s.node.watches.changed(s.root, nil) }

func (s moduleWorkspaceScope) ProtectEntry(path string) error {
	return s.node.protectEntry(s.root, path)
}

func (node *Node) projectWorkspaceRoot(ctx context.Context, project protocol.ProjectID) (string, error) {
	var root string
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		if err := store.CheckProjectAccess(ctx, db, project, false); err != nil {
			return err
		}
		return db.QueryRowContext(ctx, "SELECT path FROM worktrees WHERE project=?1 AND main=1 AND project NOT IN (SELECT project FROM removed_projects) AND id NOT IN (SELECT worktree FROM removed_worktrees)", project.String()).Scan(&root)
	})
	if errors.Is(err, sql.ErrNoRows) {
		return "", &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "project main worktree is unavailable"}
	}
	return root, err
}

func (s moduleWorkspaceScope) CaptureWorktree(id protocol.WorktreeID) (value store.ProjectWorktree, err error) {
	err = s.node.profile.Store.Node.Read(s.ctx, func(db *sql.Conn) error {
		value, err = store.ReadProjectWorktree(s.ctx, db, id)
		return err
	})
	return
}

func (s moduleWorkspaceScope) RegisterWorktree(value protocol.Worktree, adopt bool) (protocol.Output, error) {
	s.effect.external = !adopt
	s.effect.commit = func(ctx context.Context, db *sql.Conn, _ protocol.Output) (protocol.Output, []protocol.Event, error) {
		// Adopting an existing checkout changes only metadata. Keep its
		// authorization check in the same transaction as that change.
		if adopt {
			if err := s.node.authorizeCommandDB(ctx, db, s.caller, s.request); err != nil {
				return protocol.Output{}, nil, err
			}
			if err := s.node.validatePluginRequestDB(ctx, db, s.caller, s.request); err != nil {
				return protocol.Output{}, nil, err
			}
		}
		value, changes, err := store.RegisterCreatedWorktree(ctx, db, value, adopt)
		return protocol.Output{Kind: "worktree", Data: value}, changes, err
	}
	return protocol.Output{Kind: "worktree", Data: value}, nil
}

func (node *Node) captureWorktreeRemoval(ctx context.Context, id protocol.WorktreeID) (capture store.ProjectWorktree, err error) {
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		capture, err = store.PrepareWorkspaceWorktreeRemoval(ctx, db, id)
		return err
	})
	if err == nil && node.activeWorkspaceCommands(ctx, id) {
		err = &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "worktree has active commands"}
	}
	return
}

func (s moduleWorkspaceScope) RemoveWorktree(id protocol.WorktreeID, remove func(path string) error) (protocol.Output, error) {
	node := s.node
	capture, err := node.captureWorktreeRemoval(s.ctx, id)
	if err != nil {
		return protocol.Output{}, err
	}
	path := capture.Worktree.Path
	node.transfers.Remove(path)
	if err = remove(path); err != nil {
		return protocol.Output{}, err
	}
	node.watches.remove(path)
	s.effect.after = func() {
		node.removeCodeIndexes(id)
		node.knowledge.Refresh()
	}
	s.effect.external = true
	s.effect.commit = func(ctx context.Context, db *sql.Conn, out protocol.Output) (protocol.Output, []protocol.Event, error) {
		changes, err := store.FinishRemovedWorktree(ctx, db, id)
		return out, changes, err
	}
	return protocol.Output{Kind: "worktree_removed", Data: protocol.OutputWorktreeRemovedPayload{ID: id}}, nil
}
