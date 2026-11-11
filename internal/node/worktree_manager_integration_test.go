//go:build integration

package node

import (
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestWorktreeManagerCommands sends a real Node the commands of the desktop
// worktree manager, in its order, against a disposable repository: list,
// resolve a starting point, create, remove, register and clean up a stale
// registration, and the faults whose messages name a cause.
func TestWorktreeManagerCommands(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("Git executable is required")
	}
	node := acceptanceNode(t, filepath.Join(t.TempDir(), "profile"), nil, Options{})
	defer node.Stop(context.Background())
	root, commit := codeProject(t, codeFixture)
	project := acceptanceDo[protocol.Project](t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Worktree fixture", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}})
	main := codeMain(t, node, project.ID)
	parent, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	list := func() map[string]protocol.GitWorktree {
		t.Helper()
		listed := acceptanceDo[protocol.GitWorktrees](t, node, protocol.Command{Kind: "list_worktrees", Data: protocol.CommandListWorktreesPayload{Worktree: main}})
		if listed.Truncated || listed.OmittedPaths != 0 || len(listed.Entries) == 0 || !listed.Entries[0].Main || listed.Entries[0].Path != root {
			t.Fatalf("worktree listing %+v", listed)
		}
		entries := map[string]protocol.GitWorktree{}
		for _, entry := range listed.Entries {
			entries[entry.Path] = entry
		}
		return entries
	}
	create := func(path, branch, start string) (protocol.Worktree, *protocol.Fault) {
		t.Helper()
		resolved := acceptanceDo[protocol.OutputGitRevisionPayload](t, node, protocol.Command{Kind: "resolve_git_revision", Data: protocol.CommandResolveGitRevisionPayload{Worktree: main, Revision: start}})
		if resolved.Commit != commit {
			t.Fatalf("starting point %q resolved to %s, want %s", start, resolved.Commit, commit)
		}
		return acceptanceCall[protocol.Worktree](t, node, protocol.Command{Kind: "create_worktree", Data: protocol.CommandCreateWorktreePayload{Project: project.ID, Path: path, Branch: branch, Commit: resolved.Commit}})
	}
	remove := func(id protocol.WorktreeID, branch string) *protocol.Fault {
		t.Helper()
		_, failure := acceptanceCall[json.RawMessage](t, node, protocol.Command{Kind: "remove_worktree", Data: protocol.CommandRemoveWorktreePayload{Worktree: id, ExpectedHead: commit, ExpectedBranch: branch}})
		return failure
	}
	native := func(arguments ...string) {
		t.Helper()
		if output, err := exec.Command("git", append([]string{"-C", root}, arguments...)...).CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v\n%s", arguments, err, output)
		}
	}

	if entries := list(); len(entries) != 1 {
		t.Fatalf("a new project lists %d worktrees", len(entries))
	}
	if _, failure := acceptanceCall[protocol.OutputGitRevisionPayload](t, node, protocol.Command{Kind: "resolve_git_revision", Data: protocol.CommandResolveGitRevisionPayload{Worktree: main, Revision: "no-such-branch"}}); failure == nil {
		t.Fatal("an unknown starting point resolved")
	}

	feature := filepath.Join(parent, "feature")
	created, failure := create(feature, "feature", "main")
	if failure != nil || created.Path != feature || created.Main || created.Project == nil || *created.Project != project.ID {
		t.Fatalf("created worktree %+v, fault %+v", created, failure)
	}
	if entry := list()[feature]; entry.Main || entry.Locked || !entry.Available || entry.Branch == nil || *entry.Branch != "feature" || entry.Head == nil || *entry.Head != commit {
		t.Fatalf("created worktree listed as %+v", entry)
	}
	_, failure = create(filepath.Join(parent, "again"), "feature", commit[:12])
	acceptanceFault(t, "an existing branch", failure, protocol.ErrorCodeConflict, git.BranchExists)
	_, failure = create(feature, "another", "main")
	acceptanceFault(t, "an existing directory", failure, protocol.ErrorCodeConflict, git.DestinationExists)

	draft := filepath.Join(feature, "draft.txt")
	if err := os.WriteFile(draft, []byte("draft\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	acceptanceFault(t, "uncommitted changes", remove(created.ID, "feature"), protocol.ErrorCodeConflict, git.WorktreeDirty)
	if err := os.Remove(draft); err != nil {
		t.Fatal(err)
	}
	native("worktree", "lock", feature)
	if entry := list()[feature]; !entry.Locked || !entry.Available {
		t.Fatalf("locked worktree listed as %+v", entry)
	}
	acceptanceFault(t, "a locked worktree", remove(created.ID, "feature"), protocol.ErrorCodeBusy, git.WorktreeLocked)
	native("worktree", "unlock", feature)
	if failure := remove(created.ID, "feature"); failure != nil {
		t.Fatalf("remove a clean worktree: %+v", failure)
	}
	if _, err := os.Stat(feature); !os.IsNotExist(err) {
		t.Fatalf("the removed worktree's directory remains: %v", err)
	}
	if _, listed := list()[feature]; listed {
		t.Fatal("the removed worktree is still listed")
	}

	// A worktree Git knows and the Node does not is registered before it is
	// opened or removed.
	outside := filepath.Join(parent, "outside")
	native("worktree", "add", "-b", "outside", outside, commit)
	registered := acceptanceDo[protocol.Worktree](t, node, protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{Project: project.ID, Path: outside}})
	if registered.Path != outside || registered.Main {
		t.Fatalf("registered worktree %+v", registered)
	}
	if failure := remove(registered.ID, "outside"); failure != nil {
		t.Fatalf("remove a registered worktree: %+v", failure)
	}

	// A registration whose directory is gone is stale: it stays listed as
	// unavailable until it is cleaned up.
	stale := filepath.Join(parent, "stale")
	native("worktree", "add", "-b", "stale", stale, commit)
	prune := func() *protocol.Fault {
		t.Helper()
		status := acceptanceDo[protocol.GitStatus](t, node, protocol.Command{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: main}})
		index := ""
		if status.IndexRevision != nil {
			index = *status.IndexRevision
		}
		_, failure := acceptanceCall[json.RawMessage](t, node, protocol.Command{Kind: "run_git_action", Data: protocol.CommandRunGitActionPayload{
			Worktree: main, ExpectedIndex: index, ExpectedHead: status.Head, ExpectedBranch: status.Branch,
			Action: protocol.GitAction{Kind: "prune_worktree", Data: protocol.GitActionPruneWorktreePayload{Path: stale}},
		}})
		return failure
	}
	acceptanceFault(t, "a present directory", prune(), protocol.ErrorCodeConflict, git.WorktreePathExists)
	if err := os.RemoveAll(stale); err != nil {
		t.Fatal(err)
	}
	if entry, listed := list()[stale]; !listed || entry.Available || entry.Main || entry.Locked {
		t.Fatalf("stale worktree listed as %+v (%v)", entry, listed)
	}
	if failure := prune(); failure != nil {
		t.Fatalf("clean up a stale registration: %+v", failure)
	}
	if entries := list(); len(entries) != 1 {
		t.Fatalf("after cleanup %d worktrees are listed", len(entries))
	}
}
