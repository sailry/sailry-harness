package node

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorktreeCleanupCooldown(t *testing.T) {
	t.Parallel()
	var kept keptWorktrees
	first, second := protocol.NewWorktreeID(), protocol.NewWorktreeID()
	now := time.Now()
	deadline := kept.remember(first, now)
	if !kept.contains(first, now.Add(2*time.Second)) || kept.contains(second, now) {
		t.Fatal("a recent failure should defer only the same target")
	}
	kept.remember(second, deadline)
	if kept.contains(first, deadline) || !kept.contains(second, deadline) {
		t.Fatal("an expired worktree should be available for the next cleanup")
	}
}

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestRemovedSessionWorkspace(t *testing.T) {
	t.Parallel()
	profile := filepath.Join(t.TempDir(), "profile")
	node, err := startFixtureNode(context.Background(), profile)
	if err != nil {
		t.Fatal(err)
	}
	stopped := false
	t.Cleanup(func() {
		if !stopped {
			_ = node.Stop(context.Background())
		}
	})
	private := func(node *Node, text string) (protocol.Session, string) {
		t.Helper()
		session := sessionFixture(t, node)
		commandFixture(t, node, protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: session.Worktree, Path: "output.txt", Text: text}})
		root := core.SessionWorkspace(node.profile.Ownership.Path, session.ID)
		if body, err := os.ReadFile(filepath.Join(root, "output.txt")); err != nil || string(body) != text {
			t.Fatalf("private workspace file = %q, %v", body, err)
		}
		return session, root
	}
	gone := func(path string) {
		t.Helper()
		deadline := time.Now().Add(10 * time.Second)
		for {
			if _, err := os.Lstat(path); os.IsNotExist(err) {
				return
			}
			if time.Now().After(deadline) {
				t.Fatalf("%s was kept", path)
			}
			time.Sleep(5 * time.Millisecond)
		}
	}
	removed, removedRoot := private(node, "removed\n")
	archived, archivedRoot := private(node, "archived\n")
	kept, keptRoot := private(node, "kept\n")
	commandFixture(t, node, protocol.Command{Kind: "archive_sessions", Data: protocol.CommandArchiveSessionsPayload{Expected: []protocol.SessionRevision{{Session: archived.ID, Revision: archived.Revision}}}})
	commandFixture(t, node, protocol.Command{Kind: "remove_session", Data: protocol.CommandRemoveSessionPayload{Session: removed.ID, ExpectedRevision: removed.Revision}})
	gone(removedRoot)
	// Cleanup acts on removed sessions only, whichever session it is asked about.
	node.removeSessionWorkspaces([]protocol.SessionID{archived.ID, kept.ID})
	for _, root := range []string{archivedRoot, keptRoot} {
		if _, err := os.Stat(filepath.Join(root, "output.txt")); err != nil {
			t.Fatalf("a remaining session lost its workspace: %v", err)
		}
	}
	// A directory an earlier process could not remove goes at the next start.
	if err := os.MkdirAll(removedRoot, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(removedRoot, "leftover.txt"), []byte("leftover\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	stopped = true
	if err := node.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	next, err := startFixtureNode(context.Background(), profile)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = next.Stop(context.Background()) })
	gone(removedRoot)
	for _, root := range []string{archivedRoot, keptRoot} {
		if _, err := os.Stat(filepath.Join(root, "output.txt")); err != nil {
			t.Fatalf("the start sweep removed a remaining session's workspace: %v", err)
		}
	}
}

func TestModuleCannotRemovePrivateWorkspaceAsWorktree(t *testing.T) {
	handled := make(chan struct{}, 1)
	module := serviceTestWorkspaceModule{serviceTestModule: "removal-fixture", services: moduleapi.WorkspaceServices{
		Worktrees: true,
		Bind: func(*moduleapi.WorkspaceRuntime) (moduleapi.WorkspaceBindings, error) {
			return moduleapi.WorkspaceBindings{Commands: map[string]moduleapi.WorkspaceCommand{
				"remove_worktree": {Lock: moduleapi.WorkspaceLockChange, Run: func(_ context.Context, scope moduleapi.WorkspaceCommandScope, command protocol.Command) (protocol.Output, error) {
					handled <- struct{}{}
					input := command.Data.(*protocol.CommandRemoveWorktreePayload)
					return scope.RemoveWorktree(input.Worktree, func(string) error {
						return errors.New("private workspace reached the removal callback")
					})
				}},
			}}, nil
		},
	}}
	node := serviceTestNode(t, moduleapi.Assembly{Modules: []moduleapi.Module{module}})
	session := sessionFixture(t, node)
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "remove_worktree", Data: protocol.CommandRemoveWorktreePayload{
		Worktree: session.Worktree, ExpectedHead: "0000000000000000000000000000000000000000", ExpectedBranch: "main",
	}})
	result := dispatchFixture(t, node, node.ID(), request)
	if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeNotFound || len(handled) != 1 {
		t.Fatalf("private workspace did not reach the registered removal handler and fail the project capture: %+v", result)
	}
	root := core.SessionWorkspace(node.profile.Ownership.Path, session.ID)
	if _, err := os.Stat(root); err != nil {
		t.Fatalf("the private workspace was not retained: %v", err)
	}
}
