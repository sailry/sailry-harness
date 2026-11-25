package moduleapi

import (
	"context"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type WorkspaceLock uint8

const (
	// WorkspaceLockEffect holds the resolved roots to durable completion.
	WorkspaceLockEffect WorkspaceLock = iota
	// WorkspaceLockLocal leaves locking of individual effect phases to the service.
	WorkspaceLockLocal
	// WorkspaceLockChange holds the roots and advances the registration revision on release.
	WorkspaceLockChange
)

// WorkspaceCommand runs inside the same authorization, root resolution and
// durable completion path as core workspace operations.
type WorkspaceCommand struct {
	Lock WorkspaceLock
	// ProjectRoot selects the project's main workspace for commands addressed
	// to a project rather than a worktree.
	ProjectRoot func(protocol.Command) protocol.ProjectID
	Run         func(context.Context, WorkspaceCommandScope, protocol.Command) (protocol.Output, error)
}

type WorkspaceCommandScope interface {
	Root() string
	ProtectEntry(path string) error
	Changed()
	CaptureWorktree(protocol.WorktreeID) (store.ProjectWorktree, error)
	RegisterWorktree(value protocol.Worktree, adopt bool) (protocol.Output, error)
	RemoveWorktree(id protocol.WorktreeID, remove func(path string) error) (protocol.Output, error)
}

type ProjectSourcePreparer func(context.Context, string, protocol.ProjectsSource) (string, error)

type RepositoryWatchFilter interface {
	Changed([]string) bool
	ReferencesChanged([]string) bool
}

type RepositoryWatch struct {
	Open       func(context.Context, string) (RepositoryWatchFilter, []string, error)
	Invalidate func(string)
}

type WorkspaceBindings struct {
	Commands             map[string]WorkspaceCommand
	ProjectSources       map[string]ProjectSourcePreparer
	RepositoryWatch      *RepositoryWatch
	CaptureBoundary      func(root string) (string, error)
	StageTurnHunk        func(context.Context, string, protocol.ConversationCheckpointHunkSelection, func(string) (string, error)) (protocol.Output, error)
	RemoveUnusedWorktree func(context.Context, store.ProjectWorktree, func()) (bool, error)
}
