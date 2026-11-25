package node

import (
	"context"
	"errors"
	"path/filepath"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/terminal"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type serviceTestModule string

func (module serviceTestModule) ID() string             { return string(module) }
func (serviceTestModule) ReplacesPlugins() []string     { return nil }
func (serviceTestModule) Catalog() []moduleapi.ToolMeta { return nil }
func (serviceTestModule) ToolsForTurn(context.Context, moduleapi.Host, *store.AgentInvocation) ([]engine.Tool, error) {
	return nil, nil
}

type serviceTestSSH struct {
	closed  atomic.Int32
	execute func(context.Context, protocol.NodeID, protocol.Request) (protocol.Output, error)
}

func (service *serviceTestSSH) Execute(ctx context.Context, caller protocol.NodeID, request protocol.Request) (protocol.Output, error) {
	if service.execute != nil {
		return service.execute(ctx, caller, request)
	}
	return protocol.Output{}, errors.New("external SSH execution is not configured for this fixture")
}
func (*serviceTestSSH) OpenTerminal(context.Context, protocol.NodeID, protocol.Request, protocol.CommandOpenSshTerminalPayload) (protocol.SshOutcome, error) {
	return protocol.SshOutcome{}, errors.New("unexpected remote terminal")
}
func (*serviceTestSSH) Invalidate(protocol.SshID) {}
func (service *serviceTestSSH) Close() error      { service.closed.Add(1); return nil }

func serviceTestNode(t *testing.T, assembly moduleapi.Assembly) *Node {
	t.Helper()
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{Assembly: assembly, Environment: &environment, MCP: fixtureMCPOptions()})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	return node
}

func fixtureModuleAssembly(modules ...string) moduleapi.Assembly {
	var assembly moduleapi.Assembly
	for _, id := range modules {
		assembly.Modules = append(assembly.Modules, serviceTestModule(id))
		switch id {
		case "databases":
			assembly.Databases = func(context.Context, store.DatabaseOperation) (protocol.DatabaseOutcome, error) {
				return protocol.DatabaseOutcome{}, errors.New("external database execution is not configured for this fixture")
			}
		case "ssh":
			assembly.SSH = func(moduleapi.SSHServiceOptions) moduleapi.SSHService { return &serviceTestSSH{} }
		case "terminal":
			assembly.Terminal = terminal.New
		}
	}
	return assembly
}

func moduleFixtureNode(t *testing.T, modules ...string) *Node {
	t.Helper()
	return serviceTestNode(t, fixtureModuleAssembly(modules...))
}
