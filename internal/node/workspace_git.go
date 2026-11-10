package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) workspaceGitEffect(ctx context.Context, request protocol.Request, root string, effect *workspaceCompletion) (protocol.Output, error) {
	command := request.Command
	switch input := command.Data.(type) {
	case *protocol.CommandApplyGitHunkPayload:
		return node.applyManualGitHunk(ctx, root, *input)
	case *protocol.CommandCreateWorktreePayload:
		var trees []protocol.Worktree
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error { var err error; trees, err = store.ReadWorktrees(ctx, db); return err })
		if err != nil {
			return protocol.Output{}, err
		}
		for _, tree := range trees {
			if tree.Project != nil && *tree.Project == input.Project && tree.Main {
				root = tree.Path
				break
			}
		}
		if root == "" {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "project main worktree is unavailable"}
		}
		path, err := node.git.CreateWorktree(ctx, root, input.Path, input.Branch, input.Commit)
		if err != nil {
			return protocol.Output{}, err
		}
		return node.registerWorkspaceWorktree(protocol.Worktree{ID: protocol.NewWorktreeID(), Project: &input.Project, Path: path}, false, effect)
	case *protocol.CommandCreateManagedWorktreePayload:
		var capture store.WorkspaceGitCapture
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			capture, err = store.ReadWorkspaceGitCapture(ctx, db, input.Source)
			return err
		})
		if err != nil {
			return protocol.Output{}, err
		}
		if capture.Project.ID != input.Project {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "source worktree belongs to another project"}
		}
		id := protocol.NewWorktreeID()
		path, err := node.git.CreateManagedWorktree(ctx, root, capture.Project.Path, id, *input)
		if err != nil {
			return protocol.Output{}, err
		}
		return node.registerWorkspaceWorktree(protocol.Worktree{ID: id, Project: &input.Project, Path: path}, false, effect)
	case *protocol.CommandRemoveWorktreePayload:
		var capture store.WorkspaceGitCapture
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			capture, err = store.PrepareWorkspaceWorktreeRemoval(ctx, db, input.Worktree)
			return err
		})
		if err != nil {
			return protocol.Output{}, err
		}
		if node.activeWorkspaceCommands(ctx, input.Worktree) {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "worktree has active commands"}
		}
		node.transfers.Remove(capture.Worktree.Path)
		if err = node.git.RemoveWorktree(ctx, capture.Worktree.Path, input.ExpectedHead, input.ExpectedBranch); err != nil {
			return protocol.Output{}, err
		}
		node.watches.remove(capture.Worktree.Path)
		effect.external = true
		effect.commit = func(ctx context.Context, db *sql.Conn, out protocol.Output) (protocol.Output, []protocol.Event, error) {
			changes, err := store.FinishRemovedWorktree(ctx, db, input.Worktree)
			return out, changes, err
		}
		return protocol.Output{Kind: "worktree_removed", Data: protocol.OutputWorktreeRemovedPayload{ID: input.Worktree}}, nil

	default:
		node.watches.changed(root, nil)
		defer node.watches.changed(root, nil)
		return node.git.Execute(ctx, root, command)
	}
}
func (node *Node) registerWorkspaceWorktree(value protocol.Worktree, adopt bool, effect *workspaceCompletion) (protocol.Output, error) {
	effect.external = true
	effect.commit = func(ctx context.Context, db *sql.Conn, out protocol.Output) (protocol.Output, []protocol.Event, error) {
		value, changes, err := store.RegisterCreatedWorktree(ctx, db, value, adopt)
		return protocol.Output{Kind: "worktree", Data: value}, changes, err
	}
	return protocol.Output{Kind: "worktree", Data: value}, nil
}
