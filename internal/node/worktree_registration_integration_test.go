//go:build integration

package node

import (
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorktreeRegistrationTracksGitEffects(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("Git executable is required")
	}
	node := acceptanceNode(t, filepath.Join(t.TempDir(), "profile"), nil, Options{Assembly: fixtureModuleAssembly("worktrees")})
	defer node.Stop(context.Background())
	root, commit := gitProjectFixture(t, map[string]string{"file.txt": "one\n"})
	project := acceptanceDo[protocol.Project](t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Worktree fixture", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}})
	main := mainWorktreeFixture(t, node, project.ID)
	parent, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	assertStored := func(id protocol.WorktreeID, path string) {
		t.Helper()
		for _, entry := range snapshotFixture(t, node).Worktrees {
			if entry.ID == id {
				if path == "" || entry.Path != path || entry.Main || entry.Project == nil || *entry.Project != project.ID {
					t.Fatalf("stored worktree = %+v, want linked worktree at %q", entry, path)
				}
				return
			}
		}
		if path != "" {
			t.Fatalf("worktree %s was not registered", id)
		}
	}
	remove := func(id protocol.WorktreeID, branch string) *protocol.Fault {
		t.Helper()
		_, failure := acceptanceCall[json.RawMessage](t, node, protocol.Command{Kind: "remove_worktree", Data: protocol.CommandRemoveWorktreePayload{Worktree: id, ExpectedHead: commit, ExpectedBranch: branch}})
		return failure
	}
	native := func(arguments ...string) {
		t.Helper()
		cmd := exec.CommandContext(t.Context(), "git", append([]string{"-C", root}, arguments...)...)
		cmd.Env = append(os.Environ(), "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL="+os.DevNull, "GIT_TERMINAL_PROMPT=0")
		if output, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v\n%s", arguments, err, output)
		}
	}

	feature := filepath.Join(parent, "feature")
	created := acceptanceDo[protocol.Worktree](t, node, protocol.Command{Kind: "create_worktree", Data: protocol.CommandCreateWorktreePayload{Project: project.ID, Path: feature, Branch: "feature", Commit: commit}})
	assertStored(created.ID, feature)
	listed := acceptanceDo[protocol.GitWorktrees](t, node, protocol.Command{Kind: "list_worktrees", Data: protocol.CommandListWorktreesPayload{Worktree: main}})
	if len(listed.Entries) != 2 || listed.Entries[0].Path != root || listed.Entries[1].Path != feature {
		t.Fatalf("created worktree not listed: %+v", listed)
	}
	if failure := remove(created.ID, "wrong-branch"); failure == nil || failure.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("stale remove = %+v", failure)
	}
	assertStored(created.ID, feature)
	if failure := remove(created.ID, "feature"); failure != nil {
		t.Fatalf("remove a clean worktree: %+v", failure)
	}
	if _, err := os.Stat(feature); !os.IsNotExist(err) {
		t.Fatalf("the removed worktree's directory remains: %v", err)
	}
	assertStored(created.ID, "")

	// A worktree Git knows and the Node does not is registered before it is
	// opened or removed.
	outside := filepath.Join(parent, "outside")
	native("worktree", "add", "-b", "outside", outside, commit)
	registered := acceptanceDo[protocol.Worktree](t, node, protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{Project: project.ID, Path: outside}})
	assertStored(registered.ID, outside)
	if failure := remove(registered.ID, "outside"); failure != nil {
		t.Fatalf("remove a registered worktree: %+v", failure)
	}
	assertStored(registered.ID, "")
}
