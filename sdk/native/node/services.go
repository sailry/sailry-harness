package node

import (
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/terminal"
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

type CodeIndexServiceProvider = moduleapi.CodeIndexServiceProvider

var NewTerminalService = terminal.New

var ValidateTerminalViewport = terminal.ValidateViewport
var OpenSSHTerminal = terminal.OpenSSH
