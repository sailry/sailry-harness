package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) prepare(ctx context.Context, command protocol.Command) (store.PreparedPaths, error) {
	if err := node.checkCommandModule(command); err != nil {
		return store.PreparedPaths{}, err
	}
	if draft, ok := command.Data.(*protocol.ProjectsDraft); ok && command.Kind == "create_project" && draft.Source.Kind != "local" {
		if err := store.ValidateProjectDraft(*draft); err != nil {
			return store.PreparedPaths{}, err
		}
		canonical, err := node.workspace.ProjectSources[draft.Source.Kind](ctx, draft.Path, draft.Source)
		return store.PreparedPaths{Path: canonical}, err
	}
	return store.PreparePaths(command)
}

func (node *Node) checkProjectWorktreeRoots(ctx context.Context, command protocol.Command, prepared store.PreparedPaths) error {
	input, ok := command.Data.(*protocol.CommandUpdateProjectPayload)
	if !ok || prepared.Path == input.Expected.Path {
		return nil
	}
	var trees []protocol.Worktree
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		trees, err = store.ReadWorktrees(ctx, db)
		return err
	})
	if err != nil {
		return err
	}
	// Moving a project must keep its existing linked worktrees attached to
	// the same repository, including profiles reopened without workspace modules.
	for _, tree := range trees {
		if tree.Project != nil && *tree.Project == input.Expected.ID && !tree.Main {
			if err := core.VerifyWorktreeMembership(ctx, prepared.Path, tree.Path); err != nil {
				return err
			}
		}
	}
	return nil
}
