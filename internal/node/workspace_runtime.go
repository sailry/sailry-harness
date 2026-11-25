package node

import (
	"context"
	"database/sql"
	"errors"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) initWorkspace(ctx context.Context, options Options, services moduleapi.ResolvedServices) error {
	if options.Environment == nil {
		node.environment = process.CaptureEnvironment(ctx)
	} else {
		node.environment = *options.Environment
	}
	if len(services.Workspace) != 0 {
		node.workspaceRuntime = moduleapi.NewWorkspaceRuntime(moduleapi.WorkspaceOptions{
			Context: node.ctx, Environment: node.environment.TerminalValues(), Profile: node.profile.Ownership.Path,
			Lock: func(root string) func() { return node.locks.roots(root) }, Git: options.Git,
		})
		if err := node.bindWorkspace(services.Workspace); err != nil {
			return errors.Join(err, node.workspaceRuntime.Close())
		}
		node.workspaceCapabilities = services.WorkspaceCapabilities
	}
	node.watches = newWorkspaceWatches(node.workspace.RepositoryWatch)
	node.transfers = files.NewTransfers(node.profile.Ownership.Path)
	processOptions := options.Process
	node.processAfter = processOptions.After
	if node.processAfter == nil {
		node.processAfter = time.After
	}
	processOptions.Context, processOptions.Profile, processOptions.Environment = node.ctx, node.profile.Ownership.Path, node.environment
	processOptions.CaptureManaged = node.workspace.CaptureBoundary
	if processOptions.CaptureManaged == nil {
		processOptions.CaptureManaged = process.CaptureWorkspaceBoundary
	}
	processOptions.Finished = node.commandFinished
	processOptions.Stalled = node.commandStalled
	processOptions.Working = node.power.Hold
	processOptions.Changed = node.workspaceChanged
	var err error
	node.processes, err = process.New(processOptions)
	if err != nil {
		return errors.Join(err, node.transfers.Close(), node.watches.close(), node.workspaceRuntime.Close())
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
	if err == nil {
		err = node.initCodeIndex(ctx, services.CodeIndex)
	}
	if err != nil {
		_ = node.closeWorkspace()
	}
	return err
}
func (node *Node) bindWorkspace(providers []moduleapi.WorkspaceBinder) error {
	var workspace moduleapi.WorkspaceBindings
	workspace.Commands = make(map[string]moduleapi.WorkspaceCommand)
	workspace.ProjectSources = make(map[string]moduleapi.ProjectSourcePreparer)
	for _, bind := range providers {
		bindings, err := bind(node.workspaceRuntime)
		if err != nil {
			return err
		}
		for kind, handler := range bindings.Commands {
			if workspace.Commands[kind].Run == nil {
				workspace.Commands[kind] = handler
			}
		}
		for source, prepare := range bindings.ProjectSources {
			if workspace.ProjectSources[source] == nil {
				workspace.ProjectSources[source] = prepare
			}
		}
		if workspace.RepositoryWatch == nil {
			workspace.RepositoryWatch = bindings.RepositoryWatch
		}
		if workspace.CaptureBoundary == nil {
			workspace.CaptureBoundary = bindings.CaptureBoundary
		}
		if workspace.StageTurnHunk == nil {
			workspace.StageTurnHunk = bindings.StageTurnHunk
		}
		if workspace.RemoveUnusedWorktree == nil {
			workspace.RemoveUnusedWorktree = bindings.RemoveUnusedWorktree
		}
	}
	node.workspace = workspace
	return nil
}

func (node *Node) closeWorkspace() error {
	if node.web != nil {
		node.web.fetch.Close()
	}
	node.closeCodeIndex()
	return errors.Join(node.processes.Close(), node.transfers.Close(), node.watches.close(), node.workspaceRuntime.Close())
}
func (node *Node) open(ctx context.Context, caller protocol.NodeID, id protocol.StreamID) (link.Stream, error) {
	if err := node.begin(); err != nil {
		return nil, err
	}
	defer node.work.Done()
	if stream, found, err := node.ports.Open(ctx, caller, id); found || err != nil {
		return stream, err
	}
	return node.transfers.Open(ctx, caller, userFromContext(ctx).ID, id)
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
	if store.IsStorageFull(err) {
		var fault *protocol.Fault
		if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeBusy {
			// Stop the immediate retries. The next inspection can settle this
			// known-uncommitted result after the user has freed disk space.
			return errors.Join(&protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "storage is full; free disk space to record command completions"}, process.ErrCompletionStorageFull)
		}
	}
	return err
}
