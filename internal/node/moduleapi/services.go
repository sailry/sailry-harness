package moduleapi

import (
	"context"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/codeindexapi"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/node/host"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/terminal"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type DatabaseExecutor func(context.Context, store.DatabaseOperation) (protocol.DatabaseOutcome, error)

// DatabaseServiceProvider supplies the executor for the Node's database commands.
type DatabaseServiceProvider interface {
	ExecuteDatabase(context.Context, store.DatabaseOperation) (protocol.DatabaseOutcome, error)
}

type TerminalFactory func(context.Context, protocol.NodeID, func(context.Context, protocol.TerminalInfo)) *terminal.Service

// TerminalServiceProvider supplies the Node's controller terminal service.
type TerminalServiceProvider interface {
	NewTerminalService(context.Context, protocol.NodeID, func(context.Context, protocol.TerminalInfo)) *terminal.Service
}

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

// SSHServiceProvider supplies the service for the Node's SSH commands.
type SSHServiceProvider interface {
	NewSSHService(SSHServiceOptions) SSHService
}

// WorkspaceServices declares a module's capabilities and binds its handlers.
// A capability by itself does not register commands or start a service.
type WorkspaceServices struct {
	Git       bool
	Worktrees bool
	Bind      WorkspaceBinder
}

type WorkspaceServiceProvider interface {
	WorkspaceServices() WorkspaceServices
}

type CodeIndexServiceProvider interface {
	NewCodeIndexService(string, codeindexapi.Watch) codeindexapi.Service
}

type ServiceCapabilities uint8

const (
	ServiceDatabase ServiceCapabilities = 1 << iota
	ServiceSSH
	ServiceTerminal
	ServiceGit
	ServiceWorktrees
	ServiceCodeIndex
)

type RegisteredModule struct {
	Module
	Services ServiceCapabilities
}

func (m RegisteredModule) RuntimeAvailable(running ServiceCapabilities) bool {
	// Existing package requirements name a module ID, not a service family.
	// A reserved ID must still be backed by that module's declared service.
	required := legacyModuleService(m.ID())
	return required&^m.Services == 0 && m.Services&^running == 0
}

type ResolvedServices struct {
	Databases             DatabaseExecutor
	SSH                   SSHFactory
	Terminal              TerminalFactory
	CodeIndex             codeindexapi.Factory
	Workspace             []WorkspaceBinder
	WorkspaceCapabilities ServiceCapabilities
	Modules               []RegisteredModule
}

// HasModule reports presence by identity, independently of service availability.
func (a Assembly) HasModule(id string) bool {
	for _, module := range a.Modules {
		if module.ID() == id {
			return true
		}
	}
	return false
}

// ResolveServices selects the first provider of each factory in assembly order.
// A legacy factory override can accompany a typed provider or an old named
// module; alone it does not enable a service (except the standalone code index).
func (a Assembly) ResolveServices() ResolvedServices {
	services := ResolvedServices{CodeIndex: a.CodeIndex, Modules: make([]RegisteredModule, len(a.Modules))}
	var selected ServiceCapabilities
	for i, module := range a.Modules {
		var declared ServiceCapabilities
		if provider, ok := module.(DatabaseServiceProvider); ok {
			declared |= ServiceDatabase
			if services.Databases == nil {
				services.Databases = provider.ExecuteDatabase
			}
		}
		if provider, ok := module.(SSHServiceProvider); ok {
			declared |= ServiceSSH
			if services.SSH == nil {
				services.SSH = provider.NewSSHService
			}
		}
		if provider, ok := module.(TerminalServiceProvider); ok {
			declared |= ServiceTerminal
			if services.Terminal == nil {
				services.Terminal = provider.NewTerminalService
			}
		}
		if provider, ok := module.(CodeIndexServiceProvider); ok {
			declared |= ServiceCodeIndex
			if services.CodeIndex == nil {
				services.CodeIndex = provider.NewCodeIndexService
			}
		}
		if provider, ok := module.(WorkspaceServiceProvider); ok {
			workspace := provider.WorkspaceServices()
			if workspace.Bind != nil {
				services.Workspace = append(services.Workspace, workspace.Bind)
				if workspace.Git {
					declared |= ServiceGit
				}
				if workspace.Worktrees {
					declared |= ServiceWorktrees
				}
			}
		}
		services.Modules[i] = RegisteredModule{Module: module, Services: declared}
		selected |= declared
	}
	legacy := a.legacyServices()
	for i, module := range services.Modules {
		if _, typed := module.Module.(WorkspaceServiceProvider); typed || module.Services != 0 {
			continue
		}
		capability := legacyModuleService(module.ID()) & legacy &^ selected
		services.Modules[i].Services = capability
		selected |= capability
	}
	if selected&ServiceDatabase != 0 && a.Databases != nil {
		services.Databases = a.Databases
	}
	if selected&ServiceSSH != 0 && a.SSH != nil {
		services.SSH = a.SSH
	}
	if selected&ServiceTerminal != 0 && a.Terminal != nil {
		services.Terminal = a.Terminal
	}
	services.WorkspaceCapabilities = selected & (ServiceGit | ServiceWorktrees)
	return services
}

// ConnectionServices retains the connection-only view of assembly resolution.
func ConnectionServices(a Assembly) (DatabaseExecutor, SSHFactory) {
	services := a.ResolveServices()
	return services.Databases, services.SSH
}

func (a Assembly) legacyServices() ServiceCapabilities {
	var enabled ServiceCapabilities
	if a.Databases != nil {
		enabled |= ServiceDatabase
	}
	if a.SSH != nil {
		enabled |= ServiceSSH
	}
	if a.Terminal != nil {
		enabled |= ServiceTerminal
	}
	if a.CodeIndex != nil {
		enabled |= ServiceCodeIndex
	}
	return enabled
}

func legacyModuleService(id string) ServiceCapabilities {
	switch id {
	case "databases":
		return ServiceDatabase
	case "ssh":
		return ServiceSSH
	case "terminal":
		return ServiceTerminal
	case "git":
		return ServiceGit
	case "worktrees":
		return ServiceWorktrees
	case "code-index":
		return ServiceCodeIndex
	}
	return 0
}
