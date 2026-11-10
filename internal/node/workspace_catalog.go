package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) readWorkspaceCatalog(ctx context.Context, command protocol.Command) (protocol.Output, error) {
	catalog := protocol.DefaultProjectsCatalog()
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		worktrees, err := store.ReadWorktrees(ctx, db)
		if err != nil {
			return err
		}
		var captured *protocol.Worktree
		if command.Kind == "read_worktree_catalog" {
			id := command.Data.(*protocol.CommandReadWorktreeCatalogPayload).Worktree
			for index := range worktrees {
				if worktrees[index].ID == id {
					captured = &worktrees[index]
					break
				}
			}
			if captured == nil {
				return &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "captured worktree is unavailable"}
			}
		}
		projects, err := store.ReadProjects(ctx, db)
		if err != nil {
			return err
		}
		for _, project := range projects {
			if captured == nil || captured.Project != nil && *captured.Project == project.ID {
				catalog.Projects = append(catalog.Projects, project)
			}
		}
		for _, worktree := range worktrees {
			if captured != nil {
				if worktree.ID == captured.ID || captured.Project != nil && worktree.Project != nil && *worktree.Project == *captured.Project {
					catalog.Worktrees = append(catalog.Worktrees, worktree)
				}
				continue
			}
			for _, project := range catalog.Projects {
				if worktree.Project != nil && *worktree.Project == project.ID {
					catalog.Worktrees = append(catalog.Worktrees, worktree)
					break
				}
			}
		}
		return nil
	})
	return protocol.Output{Kind: "project_catalog", Data: catalog}, err
}
