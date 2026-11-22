package codeindex_test

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/modules/code-index/node"
)

func indexNodeFixture(t *testing.T) (*node.Node, protocol.WorktreeID) {
	t.Helper()
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	n, err := node.StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), node.Options{Environment: &environment, CodeIndex: codeindex.NewService})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := n.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "main.go"), []byte("package main\n\nfunc main() {}\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	project := indexCommand(t, n, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Code fixture", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	snapshot, err := n.Snapshot(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	for _, tree := range snapshot.Worktrees {
		if tree.Project != nil && *tree.Project == project.ID && tree.Main {
			return n, tree.ID
		}
	}
	t.Fatal("project has no main worktree")
	return nil, protocol.WorktreeID{}
}

func indexCommand(t *testing.T, n *node.Node, command protocol.Command) protocol.Output {
	t.Helper()
	result := dispatchIndexCommand(t, n, command)
	if result.Fault != nil {
		t.Fatalf("%s: %v", command.Kind, result.Fault)
	}
	return result.Output
}

func dispatchIndexCommand(t *testing.T, n *node.Node, command protocol.Command) link.Response {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
	defer cancel()
	caller := n.ID()
	admitted, err := n.Dispatch(ctx, caller, protocol.NewRequest(caller, command))
	if err != nil {
		var fault *protocol.Fault
		if errors.As(err, &fault) {
			return link.Response{Fault: fault}
		}
		t.Fatal(err)
	}
	select {
	case result, ok := <-admitted.Completion:
		if !ok {
			t.Fatalf("%s: command completion was lost", command.Kind)
		}
		return result
	case <-ctx.Done():
		t.Fatalf("%s: %v", command.Kind, ctx.Err())
	}
	return link.Response{}
}
