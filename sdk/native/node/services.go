package node

import (
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/terminal"
)

type DatabaseOperation = store.DatabaseOperation

type DatabaseExecutor = moduleapi.DatabaseExecutor

type SSHServiceOptions = moduleapi.SSHServiceOptions

type SSHService = moduleapi.SSHService

type SSHFactory = moduleapi.SSHFactory

type TerminalFactory = moduleapi.TerminalFactory

type TerminalService = terminal.Service

type TerminalLaunch = terminal.Launch

var NewTerminalService = terminal.New

var ValidateTerminalViewport = terminal.ValidateViewport
var OpenSSHTerminal = terminal.OpenSSH
