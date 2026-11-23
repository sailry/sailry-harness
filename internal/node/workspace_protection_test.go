package node

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestFileMutationProtection keeps writes out of registered worktrees and Node
// storage while a conversation without a project writes its managed workspace.
func TestFileMutationProtection(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	config := sessionConfigFixture(t, node)
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &config}}).Data.(protocol.Session)
	write := func(worktree protocol.WorktreeID, path string) *protocol.Fault {
		t.Helper()
		return dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "write_file", Data: &protocol.CommandWriteFilePayload{Worktree: worktree, Path: path, Text: "text\n"}})).Fault
	}
	if fault := write(session.Worktree, "notes.txt"); fault != nil {
		t.Fatalf("managed workspace write: %v", fault)
	}
	workspace := core.SessionWorkspace(node.profile.Ownership.Path, session.ID)
	if content, err := os.ReadFile(filepath.Join(workspace, "notes.txt")); err != nil || string(content) != "text\n" {
		t.Fatalf("managed workspace file = %q, %v", content, err)
	}

	outer, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	inner := filepath.Join(outer, "inner")
	if err := os.Mkdir(inner, 0o700); err != nil {
		t.Fatal(err)
	}
	var trees []protocol.WorktreeID
	for _, path := range []string{outer, inner} {
		project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
			Name: filepath.Base(path), Path: path, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
		}}).Data.(protocol.Project)
		for _, tree := range snapshotFixture(t, node).Worktrees {
			if tree.Project != nil && *tree.Project == project.ID && tree.Main {
				trees = append(trees, tree.ID)
			}
		}
	}
	if len(trees) != 2 {
		t.Fatalf("project worktrees = %v", trees)
	}
	if fault := write(trees[0], "inner"); fault == nil || fault.Code != protocol.ErrorCodeConflict {
		t.Fatalf("write over a registered worktree: %v", fault)
	}
	if fault := write(trees[0], "outer.txt"); fault != nil {
		t.Fatalf("project write: %v", fault)
	}
}
