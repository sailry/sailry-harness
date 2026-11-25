package node

import (
	"database/sql"
	"log/slog"
	"maps"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const worktreeCleanupCooldown = time.Minute

// Concurrent session removals and the startup sweep can select the same target.
// Only automatic cleanup consults this state; no timer schedules another removal.
type keptWorktrees struct {
	mu        sync.Mutex
	until     map[protocol.WorktreeID]time.Time
	nextPrune time.Time
}

func (kept *keptWorktrees) contains(id protocol.WorktreeID, now time.Time) bool {
	kept.mu.Lock()
	defer kept.mu.Unlock()
	return now.Before(kept.until[id])
}

func (kept *keptWorktrees) remember(id protocol.WorktreeID, now time.Time) time.Time {
	kept.mu.Lock()
	defer kept.mu.Unlock()
	until := now.Add(worktreeCleanupCooldown)
	if kept.until == nil {
		kept.until = make(map[protocol.WorktreeID]time.Time)
	}
	if !now.Before(kept.nextPrune) {
		maps.DeleteFunc(kept.until, func(_ protocol.WorktreeID, deadline time.Time) bool { return !now.Before(deadline) })
		kept.nextPrune = until
	}
	kept.until[id] = until
	return until
}

// removeSessionWorkspaces removes what removed sessions alone used. The
// private workspace of a session without a project goes with the session. A
// managed Git worktree goes when Git removes it as it would for the user:
// clean, on a branch, unlocked and with no operation under way. Any other
// managed worktree is kept for the worktree manager, where it shows as unused.
// Archiving removes nothing.
func (node *Node) removeSessionWorkspaces(sessions []protocol.SessionID) {
	if node.begin() != nil {
		return
	}
	defer node.work.Done()
	var leftovers []store.SessionLeftover
	err := node.profile.Store.Node.Read(node.ctx, func(db *sql.Conn) error {
		var err error
		leftovers, err = store.ReadSessionLeftovers(node.ctx, db, sessions)
		return err
	})
	if err != nil {
		if node.ctx.Err() == nil {
			slog.Error("session workspace cleanup failed", "error", err)
		}
		return
	}
	profile := node.profile.Ownership.Path
	attempted := make(map[protocol.WorktreeID]bool)
	for _, leftover := range leftovers {
		switch tree := leftover.Worktree; {
		case tree.Project == nil && tree.Path == core.SessionWorkspace(profile, leftover.Session):
			node.removePrivateWorkspace(leftover.Session, tree.Path)
		case tree.Project != nil && !attempted[tree.ID]:
			attempted[tree.ID] = true
			node.removeUnusedWorktree(tree)
		}
	}
}

func (node *Node) removePrivateWorkspace(session protocol.SessionID, path string) {
	node.transfers.Remove(path)
	node.watches.remove(path)
	parent, err := os.OpenRoot(filepath.Dir(path))
	if err == nil {
		err = parent.RemoveAll(session.String())
		_ = parent.Close()
	}
	// A directory still in use stays for the sweep at the next start.
	if err != nil && !os.IsNotExist(err) {
		slog.Warn("session workspace kept", "session", session.String(), "error", err)
	}
}

func (node *Node) removeUnusedWorktree(tree protocol.Worktree) {
	remove := node.workspace.RemoveUnusedWorktree
	if remove == nil {
		return
	}
	ctx := node.ctx
	// Keep the checkout and its registration ordered against workspace effects.
	release := node.locks.change(tree.Path)
	defer release()
	if node.keptWorktrees.contains(tree.ID, time.Now()) {
		return
	}
	capture, err := node.captureWorktreeRemoval(ctx, tree.ID)
	if err != nil || capture.Worktree.Path != tree.Path {
		return
	}
	path := capture.Worktree.Path
	removed, err := remove(ctx, capture, func() { node.transfers.Remove(path) })
	if err != nil {
		until := node.keptWorktrees.remember(tree.ID, time.Now())
		slog.Info("managed worktree kept", "worktree", tree.ID.String(), "path", path, "reason", err, "auto_cleanup_not_before", until)
		return
	}
	if !removed {
		return
	}
	node.watches.remove(path)
	// The commit and its frame are ordered with every other publication.
	unlock := node.agent.LockPublication()
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		changes, err := store.FinishRemovedWorktree(ctx, db, tree.ID)
		if err == nil {
			_, err = store.RecordEvents(ctx, db, node.ID(), changes)
		}
		return err
	})
	if err == nil {
		node.publish()
	}
	unlock()
	if err != nil {
		if ctx.Err() == nil {
			slog.Error("removed worktree was not recorded", "worktree", tree.ID.String(), "error", err)
		}
		return
	}
	node.removeCodeIndexes(tree.ID)
}

// sweepSessionWorkspaces removes the private workspaces of removed sessions
// that an earlier process left behind.
func (node *Node) sweepSessionWorkspaces() {
	entries, err := os.ReadDir(filepath.Dir(core.SessionWorkspace(node.profile.Ownership.Path, protocol.SessionID{})))
	if err != nil {
		return
	}
	sessions := []protocol.SessionID{}
	for _, entry := range entries {
		if session, err := protocol.ParseSessionID(entry.Name()); err == nil && entry.IsDir() {
			sessions = append(sessions, session)
		}
	}
	if len(sessions) > 0 {
		go node.removeSessionWorkspaces(sessions)
	}
}
