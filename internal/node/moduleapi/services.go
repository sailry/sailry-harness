package moduleapi

import (
	"context"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/node/host"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/terminal"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type DatabaseExecutor func(context.Context, store.DatabaseOperation) (protocol.DatabaseOutcome, error)

type TerminalFactory func(context.Context, protocol.NodeID, func(context.Context, protocol.TerminalInfo)) *terminal.Service

type SSHServiceOptions struct {
	Context       context.Context
	ReadSSH       func(context.Context, protocol.SshID, uint64) (protocol.SshProfile, protocol.SshCredential, error)
	WorkspaceRoot func(context.Context, protocol.WorktreeID, bool) (string, error)
	Transfers     *files.Transfers
	Terminals     *terminal.Service
	Installations *host.Installations
	Link          func() *link.Link
}

type SSHService interface {
	Execute(context.Context, protocol.NodeID, protocol.Request) (protocol.Output, error)
	OpenTerminal(context.Context, protocol.NodeID, protocol.Request, protocol.CommandOpenSshTerminalPayload) (protocol.SshOutcome, error)
	Invalidate(protocol.SshID)
	Close() error
}

type SSHFactory func(SSHServiceOptions) SSHService

func (a Assembly) HasModule(id string) bool {
	for _, module := range a.Modules {
		if module.ID() == id {
			return true
		}
	}
	return false
}
