package node

import (
	"database/sql"
	"log/slog"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

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
	for _, leftover := range leftovers {
		switch tree := leftover.Worktree; {
		case tree.Project == nil && tree.Path == core.SessionWorkspace(profile, leftover.Session):
			node.removePrivateWorkspace(leftover.Session, tree.Path)
		case tree.Project != nil:
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
	ctx := node.ctx
	// Removing a worktree changes which roots exist, as remove_worktree does,
	// and like it holds the root from Git's local work to the row: the root
	// goes on disk before its row, and no effect may run in it in between.
	// A project command waits too, so the path read below stays the root.
	release := node.locks.change(tree.Path)
	defer release()
	var capture store.WorkspaceGitCapture
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		capture, err = store.PrepareWorkspaceWorktreeRemoval(ctx, db, tree.ID)
		return err
	})
	if err != nil || capture.Worktree.Path != tree.Path || capture.Worktree.Path != filepath.Join(core.Worktrees(capture.Project.Path), tree.ID.String()) || node.activeWorkspaceCommands(ctx, tree.ID) {
		return
	}
	path := capture.Worktree.Path
	status, err := node.git.Status(ctx, path)
	if err != nil || status.Head == nil || status.Branch == nil {
		return
	}
	node.transfers.Remove(path)
	if err := node.git.RemoveWorktree(ctx, path, *status.Head, *status.Branch); err != nil {
		slog.Info("managed worktree kept", "worktree", tree.ID.String(), "reason", err)
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
