package node

import (
	"context"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/terminal"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type DatabaseOperation = store.DatabaseOperation

type DatabaseExecutor = moduleapi.DatabaseExecutor

type DatabaseServiceProvider = moduleapi.DatabaseServiceProvider

type SSHServiceOptions = moduleapi.SSHServiceOptions

type SSHService = moduleapi.SSHService

type SSHFactory = moduleapi.SSHFactory

type SSHServiceProvider = moduleapi.SSHServiceProvider

type TerminalFactory = moduleapi.TerminalFactory

type TerminalServiceProvider = moduleapi.TerminalServiceProvider

type TerminalService = terminal.Service

type TerminalLaunch = terminal.Launch

type WorkspaceServices = moduleapi.WorkspaceServices

type WorkspaceServiceProvider = moduleapi.WorkspaceServiceProvider

type GitOptions = moduleapi.GitOptions
type GitRunner = moduleapi.GitRunner
type GitCommandResult = moduleapi.GitCommandResult
type WorkspaceOptions = moduleapi.WorkspaceOptions
type WorkspaceRuntime = moduleapi.WorkspaceRuntime
type WorkspaceBinder = moduleapi.WorkspaceBinder
type WorkspaceFactory[T any] = moduleapi.WorkspaceFactory[T]
type WorkspaceBindings = moduleapi.WorkspaceBindings
type WorkspaceCommand = moduleapi.WorkspaceCommand
type WorkspaceLock = moduleapi.WorkspaceLock
type WorkspaceCommandScope = moduleapi.WorkspaceCommandScope
type ProjectWorktree = store.ProjectWorktree
type ProjectSourcePreparer = moduleapi.ProjectSourcePreparer
type RepositoryWatch = moduleapi.RepositoryWatch
type RepositoryWatchFilter = moduleapi.RepositoryWatchFilter

const (
	WorkspaceLockEffect = moduleapi.WorkspaceLockEffect
	WorkspaceLockLocal  = moduleapi.WorkspaceLockLocal
	WorkspaceLockChange = moduleapi.WorkspaceLockChange
)

func WorkspaceDirectory(path string) (string, error) { return store.WorkspaceDirectory(path) }

func VerifyWorktreeMembership(ctx context.Context, root, path string) error {
	return core.VerifyWorktreeMembership(ctx, root, path)
}

func ManagedWorktreePath(projectRoot string, id protocol.WorktreeID) string {
	return filepath.Join(core.Worktrees(projectRoot), id.String())
}

type CodeIndexServiceProvider = moduleapi.CodeIndexServiceProvider

var NewTerminalService = terminal.New

var ValidateTerminalViewport = terminal.ValidateViewport
var OpenSSHTerminal = terminal.OpenSSH
