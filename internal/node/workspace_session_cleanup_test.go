package node

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

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
	// Removing a worktree holds its own root until the command ends, not
	// every effect on the Node: Git may take long.
	change := &workspaceCompletion{}
	removal := protocol.NewRequest(node.ID(), protocol.Command{Kind: "remove_worktree", Data: &protocol.CommandRemoveWorktreePayload{Worktree: kept.Worktree}})
	if _, err := node.workspaceIO(context.Background(), node.ID(), removal, nil, change); err == nil {
		change.close()
		t.Fatal("a private workspace was removed as a project worktree")
	}
	other := make(chan func(), 1)
	go func() { other <- node.locks.effect(archivedRoot) }()
	select {
	case release := <-other:
		release()
		change.close()
	case <-time.After(10 * time.Second):
		change.close()
		(<-other)()
		t.Fatal("removing a worktree stalled an effect in another root")
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
