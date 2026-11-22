//go:build integration

package node

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
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
		config := sessionConfigFixture(t, node)
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

	t.Run("one attempt per protected worktree", func(t *testing.T) {
		protected, other := managed("submodules"), managed("batch-clean")
		module, _ := codeProject(t, map[string]string{"file": "module contents"})
		git := func(args ...string) {
			t.Helper()
			cmd := exec.Command("git", args...)
			cmd.Dir = protected.Path
			cmd.Env = append(os.Environ(), "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL="+os.DevNull, "GIT_TERMINAL_PROMPT=0")
			if out, err := cmd.CombinedOutput(); err != nil {
				t.Fatalf("fixture git %s: %v\n%s", args[0], err, out)
			}
		}
		git("clone", "--no-local", "--", module, "module")
		git("submodule", "add", "--", module, "module")
		git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.test", "commit", "-m", "Add submodule")
		a, b, c := session(protected), session(protected), session(other)
		acceptanceDo[[]protocol.SessionID](t, node, protocol.Command{Kind: "remove_sessions", Data: protocol.CommandRemoveSessionsPayload{Expected: []protocol.SessionRevision{
			{Session: a.ID, Revision: a.Revision}, {Session: b.ID, Revision: b.Revision}, {Session: c.ID, Revision: c.Revision},
		}}})
		attempts := func() int {
			return strings.Count(node.git.Output(root), "> git worktree remove -- "+protected.Path+"\n")
		}
		acceptanceEventually(t, "the other worktree is collected", func() bool { return !registered(other.ID) && attempts() > 0 })
		if got := attempts(); got != 1 || !registered(protected.ID) {
			t.Fatalf("shared submodule worktree: %d Git removal attempts, registered=%t", got, registered(protected.ID))
		}
		node.removeSessionWorkspaces([]protocol.SessionID{a.ID, b.ID})
		if got := attempts(); got != 1 {
			t.Fatalf("an overlapping cleanup retried the same target: %d Git removal attempts", got)
		}
		status, err := node.git.Status(t.Context(), protected.Path)
		if err != nil || status.Head == nil || status.Branch == nil {
			t.Fatalf("protected checkout has no branch or HEAD: %v", err)
		}
		manual := protocol.Command{Kind: "remove_worktree", Data: protocol.CommandRemoveWorktreePayload{Worktree: protected.ID, ExpectedHead: *status.Head, ExpectedBranch: *status.Branch}}
		_, failure := acceptanceCall[json.RawMessage](t, node, manual)
		if failure == nil || failure.Code != protocol.ErrorCodeConflict || !strings.Contains(failure.Message, "submodules") {
			t.Fatalf("manual removal must report Git's submodule protection: %v", failure)
		}
		if body, err := os.ReadFile(filepath.Join(protected.Path, "module", "file")); err != nil || string(body) != "module contents" {
			t.Fatalf("submodule checkout not preserved: %q, %v", body, err)
		}
		// Preserve the independent submodule checkout outside the worktree, as a user could.
		if err := os.Rename(filepath.Join(protected.Path, "module"), filepath.Join(t.TempDir(), "preserved")); err != nil {
			t.Fatal(err)
		}
		if err := os.Mkdir(filepath.Join(protected.Path, "module"), 0o700); err != nil {
			t.Fatal(err)
		}
		acceptanceDo[json.RawMessage](t, node, manual)
		if got := attempts(); got != 3 || registered(protected.ID) {
			t.Fatalf("manual removal after preserving the submodule: %d total attempts, registered=%t", got, registered(protected.ID))
		}
	})
}
