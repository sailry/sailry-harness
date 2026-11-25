//go:build integration

package node

import (
	"context"
	"errors"
	"os"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSessionCleanupDelegatesUnusedWorktrees(t *testing.T) {
	var mu sync.Mutex
	counts := make(map[protocol.WorktreeID]int)
	var kept protocol.WorktreeID
	module := serviceTestWorkspaceModule{serviceTestModule: "cleanup-fixture", services: moduleapi.WorkspaceServices{
		Worktrees: true,
		Bind: func(*moduleapi.WorkspaceRuntime) (moduleapi.WorkspaceBindings, error) {
			return moduleapi.WorkspaceBindings{RemoveUnusedWorktree: func(_ context.Context, capture store.ProjectWorktree, beforeRemove func()) (bool, error) {
				mu.Lock()
				defer mu.Unlock()
				counts[capture.Worktree.ID]++
				if capture.Worktree.ID == kept {
					return false, errors.New("fixture refused to remove checkout")
				}
				beforeRemove()
				err := os.Remove(capture.Worktree.Path)
				return err == nil, err
			}}, nil
		},
	}}
	node := serviceTestNode(t, moduleapi.Assembly{Modules: []moduleapi.Module{module}})
	project := acceptanceDo[protocol.Project](t, node, protocol.Command{Kind: "register_project", Data: protocol.CommandRegisterProjectPayload{Name: "Cleanup fixture", Path: t.TempDir()}})
	clean := recordWorkspaceFixture(t, node, project.ID, t.TempDir())
	protected := recordWorkspaceFixture(t, node, project.ID, t.TempDir())
	shared := recordWorkspaceFixture(t, node, project.ID, t.TempDir())
	mu.Lock()
	kept = protected.ID
	mu.Unlock()
	config := sessionConfigFixture(t, node)
	session := func(tree protocol.Worktree) protocol.Session {
		t.Helper()
		return acceptanceDo[protocol.Session](t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Worktree: &tree.ID, Config: &config}})
	}
	cleanSession, a, b, first, second := session(clean), session(protected), session(protected), session(shared), session(shared)
	remove := func(values ...protocol.Session) {
		t.Helper()
		var expected []protocol.SessionRevision
		for _, value := range values {
			expected = append(expected, protocol.SessionRevision{Session: value.ID, Revision: value.Revision})
		}
		acceptanceDo[[]protocol.SessionID](t, node, protocol.Command{Kind: "remove_sessions", Data: protocol.CommandRemoveSessionsPayload{Expected: expected}})
	}
	registered := func(id protocol.WorktreeID) bool {
		for _, tree := range snapshotFixture(t, node).Worktrees {
			if tree.ID == id {
				return true
			}
		}
		return false
	}
	attempts := func(id protocol.WorktreeID) int {
		mu.Lock()
		defer mu.Unlock()
		return counts[id]
	}
	remove(a, b, first, cleanSession)
	acceptanceEventually(t, "the clean checkout is collected and the protected one is attempted", func() bool {
		return !registered(clean.ID) && attempts(protected.ID) != 0
	})
	node.removeSessionWorkspaces([]protocol.SessionID{a.ID, b.ID, first.ID})
	if attempts(protected.ID) != 1 || !registered(protected.ID) {
		t.Fatal("overlapping cleanup retried or unregistered a checkout the provider kept")
	}
	if attempts(shared.ID) != 0 || !registered(shared.ID) {
		t.Fatal("cleanup reached a checkout still used by another session")
	}
	if _, err := os.Stat(clean.Path); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("collected checkout remains on disk: %v", err)
	}
	remove(second)
	acceptanceEventually(t, "the shared checkout is collected with its last session", func() bool { return !registered(shared.ID) })
	if attempts(shared.ID) != 1 {
		t.Fatalf("shared checkout was attempted %d times", attempts(shared.ID))
	}
}
