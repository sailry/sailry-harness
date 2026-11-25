package live

import (
	"context"

	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/terminal"
)

// Projection fixtures exercise the shared Node services and live adapters, not
// a product's agent tools. Select those services without changing Harness defaults.
func panelProjectionHost() moduleapi.Assembly {
	assembly := hostassembly.Harness()
	assembly.Terminal = terminal.New
	assembly.Modules = append(assembly.Modules, projectionServiceModule("git"), projectionServiceModule("terminal"))
	return assembly
}

type projectionServiceModule string

func (module projectionServiceModule) ID() string         { return string(module) }
func (projectionServiceModule) ReplacesPlugins() []string { return nil }
func (projectionServiceModule) Catalog() []moduleapi.ToolMeta {
	return nil
}
func (projectionServiceModule) ToolsForTurn(context.Context, moduleapi.Host, *store.AgentInvocation) ([]engine.Tool, error) {
	return nil, nil
}
