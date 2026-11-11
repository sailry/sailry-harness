//go:build integration

package node

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// A managed worktree goes with its last session only when Git removes it as
// clean; one with uncommitted work, or still used by a session, stays.
func TestRemovedSessionManagedWorktree(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("Git executable is required")
	}
	node := acceptanceNode(t, filepath.Join(t.TempDir(), "profile"), nil, Options{})
	defer node.Stop(context.Background())
	root, commit := codeProject(t, codeFixture)
	project := acceptanceDo[protocol.Project](t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Session worktrees", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}})
	main := codeMain(t, node, project.ID)
	managed := func(branch string) protocol.Worktree {
		t.Helper()
		index, err := os.ReadFile(filepath.Join(root, ".git", "index"))
		if err != nil {
			t.Fatal(err)
		}
		tree := acceptanceDo[protocol.Worktree](t, node, protocol.Command{Kind: "create_managed_worktree", Data: protocol.CommandCreateManagedWorktreePayload{
			Project: project.ID, Source: main, Branch: branch, ExpectedHead: commit, ExpectedIndex: fmt.Sprintf("%x", blake3.Sum256(index)),
		}})
		if tree.Path != filepath.Join(core.Worktrees(root), tree.ID.String()) {
			t.Fatalf("managed worktree at %s", tree.Path)
		}
		return tree
	}
	session := func(tree protocol.Worktree) protocol.Session {
		t.Helper()
		config := protocol.DefaultSessionConfig()
		config.Provider = protocol.NewProviderID()
		config.Model = "fixture"
		config.Mode = protocol.WorkModeCode
		config.Permission = protocol.PermissionProject
		return acceptanceDo[protocol.Session](t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Worktree: &tree.ID, Config: &config}})
	}
	remove := func(value protocol.Session) {
		t.Helper()
		acceptanceDo[[]protocol.SessionID](t, node, protocol.Command{Kind: "remove_session", Data: protocol.CommandRemoveSessionPayload{Session: value.ID, ExpectedRevision: value.Revision}})
	}
	registered := func(id protocol.WorktreeID) bool {
		t.Helper()
		for _, tree := range snapshotFixture(t, node).Worktrees {
			if tree.ID == id {
				return true
			}
		}
		return false
	}
	clean, dirty, shared := managed("clean"), managed("dirty"), managed("shared")
	cleanSession, dirtySession, first, second := session(clean), session(dirty), session(shared), session(shared)
	if err := os.WriteFile(filepath.Join(dirty.Path, "draft.txt"), []byte("uncommitted\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	remove(first)
	remove(dirtySession)
	remove(cleanSession)
	acceptanceEventually(t, "the clean worktree of a removed session goes", func() bool {
		_, err := os.Lstat(clean.Path)
		return os.IsNotExist(err) && !registered(clean.ID)
	})
	// Decide the other two here, whatever their own cleanup has reached.
	node.removeSessionWorkspaces([]protocol.SessionID{dirtySession.ID, first.ID})
	if body, err := os.ReadFile(filepath.Join(dirty.Path, "draft.txt")); err != nil || string(body) != "uncommitted\n" || !registered(dirty.ID) {
		t.Fatalf("uncommitted work of a removed session: %q, %v", body, err)
	}
	if _, err := os.Stat(shared.Path); err != nil || !registered(shared.ID) {
		t.Fatalf("a worktree another session uses was removed: %v", err)
	}
	// The branch of the removed worktree keeps its commits.
	if output, err := exec.Command("git", "-C", root, "rev-parse", "--verify", "refs/heads/clean").CombinedOutput(); err != nil {
		t.Fatalf("branch of the removed worktree: %v\n%s", err, output)
	}
	remove(second)
	acceptanceEventually(t, "a worktree goes with its last session", func() bool {
		_, err := os.Lstat(shared.Path)
		return os.IsNotExist(err) && !registered(shared.ID)
	})
}
