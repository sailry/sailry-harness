package node

import (
	"context"
	"database/sql"
	"os"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type fixtureWatchFilter struct{}

func (fixtureWatchFilter) Changed([]string) bool           { return true }
func (fixtureWatchFilter) ReferencesChanged([]string) bool { return false }

func fixtureRepositoryWatch() *moduleapi.RepositoryWatch {
	return &moduleapi.RepositoryWatch{
		Open: func(context.Context, string) (moduleapi.RepositoryWatchFilter, []string, error) {
			return fixtureWatchFilter{}, nil, nil
		},
		Invalidate: func(string) {},
	}
}

func fixtureGitBindings(*moduleapi.WorkspaceRuntime) (moduleapi.WorkspaceBindings, error) {
	return moduleapi.WorkspaceBindings{RepositoryWatch: fixtureRepositoryWatch(), Commands: map[string]moduleapi.WorkspaceCommand{
		"inspect_git": {Run: func(_ context.Context, scope moduleapi.WorkspaceCommandScope, _ protocol.Command) (protocol.Output, error) {
			_, err := os.Stat(scope.Root())
			return protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindDirectory}}, err
		}},
	}}, nil
}

func fixtureWorktreeBindings(*moduleapi.WorkspaceRuntime) (moduleapi.WorkspaceBindings, error) {
	return moduleapi.WorkspaceBindings{Commands: map[string]moduleapi.WorkspaceCommand{
		"list_worktrees": {Run: func(_ context.Context, scope moduleapi.WorkspaceCommandScope, _ protocol.Command) (protocol.Output, error) {
			_, err := os.Stat(scope.Root())
			return protocol.Output{Kind: "git_worktrees", Data: protocol.GitWorktrees{Kind: protocol.RepositoryKindDirectory}}, err
		}},
	}}, nil
}

func recordWorkspaceFixture(t *testing.T, node *Node, project protocol.ProjectID, path string) protocol.Worktree {
	t.Helper()
	root, err := store.WorkspaceDirectory(path)
	if err != nil {
		t.Fatal(err)
	}
	ctx := t.Context()
	value := protocol.Worktree{ID: protocol.NewWorktreeID(), Project: &project, Path: root}
	unlock := node.agent.LockPublication()
	defer unlock()
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		_, changes, err := store.RegisterCreatedWorktree(ctx, db, value, false)
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(ctx, db, node.ID(), changes)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	node.publish()
	return value
}
