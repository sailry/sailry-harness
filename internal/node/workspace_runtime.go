package node

import (
	"context"
	"database/sql"
	"errors"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/files"
	workspacegit "github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) initWorkspace(ctx context.Context, options Options) error {
	if options.Environment == nil {
		node.environment = process.CaptureEnvironment(ctx)
	} else {
		node.environment = *options.Environment
	}
	gitOptions := options.Git
	if gitOptions.Environment == nil {
		gitOptions.Environment = node.environment.TerminalValues()
	}
	node.git = workspacegit.New(gitOptions)
	node.watches = newWorkspaceWatches(node.git)
	node.transfers = files.NewTransfers(node.profile.Ownership.Path)
	processOptions := options.Process
	node.processAfter = processOptions.After
	if node.processAfter == nil {
		node.processAfter = time.After
	}
	processOptions.Context, processOptions.Profile, processOptions.Environment = node.ctx, node.profile.Ownership.Path, node.environment
	processOptions.CaptureManaged = node.git.CaptureManaged
	processOptions.Finished = node.commandFinished
	processOptions.Changed = node.workspaceChanged
	var err error
	node.processes, err = process.New(processOptions)
	if err != nil {
		_ = node.transfers.Close()
		_ = node.watches.close()
		return err
	}
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		changes, e := store.RecoverCommands(ctx, db, time.Now())
		if e != nil {
			return e
		}
		_, e = store.RecordEvents(ctx, db, node.ID(), changes)
		return e
	})
	if err == nil {
		err = node.initWeb(ctx, options)
	}
	if err != nil {
		_ = node.closeWorkspace()
	}
	return err
}
func (node *Node) closeWorkspace() error {
	if node.web != nil {
		node.web.fetch.Close()
	}
	return errors.Join(node.processes.Close(), node.transfers.Close(), node.watches.close())
}
func (node *Node) Open(ctx context.Context, caller protocol.NodeID, id protocol.StreamID) (link.Stream, error) {
	if err := node.begin(); err != nil {
		return nil, err
	}
	defer node.work.Done()
	return node.transfers.Open(ctx, caller, id)
}
func (node *Node) workspaceChanged(worktree protocol.WorktreeID) {
	var root string
	err := node.profile.Store.Node.Read(context.WithoutCancel(node.ctx), func(db *sql.Conn) error {
		var err error
		root, err = store.ReadWorkspaceRoot(context.WithoutCancel(node.ctx), db, worktree, true)
		return err
	})
	if err == nil {
		node.watches.changed(root, nil)
	}
}
func (node *Node) commandFinished(ctx context.Context, finished process.Finished) error {
	unlock := node.agent.LockPublication()
	now := time.Now()
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		changes, err := store.FinishCommand(ctx, db, finished, now)
		if err != nil {
			return err
		}
		_, scheduled, err := store.ScheduleBackground(ctx, db, node.ID(), finished.Snapshot.Info.Session, now)
		if err != nil {
			return err
		}
		changes = append(changes, scheduled...)
		_, err = store.RecordEvents(ctx, db, node.ID(), changes)
		return err
	})
	if err == nil {
		node.publish()
		node.agent.RefreshCommitted(finished.Snapshot.Info.Session)
	}
	unlock()
	if err == nil {
		node.agent.Committed()
	}
	return err
}
