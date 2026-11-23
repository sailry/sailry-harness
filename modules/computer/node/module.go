// Package computernode provides the compile-time native Node module for
// desktop automation via the bundled agent-desktop MCP helper.
package computernode

import (
	"context"
	_ "embed"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
)

//go:embed tools.json
var catalogJSON []byte

var loadCatalog = sync.OnceValues(func() (moduleapi.CatalogSpec, error) {
	return moduleapi.LoadCatalog(catalogJSON)
})

type module struct{}

// Module returns the native Node module providing desktop_* computer-use tools.
func Module() moduleapi.Module { return module{} }

func (module) ID() string                { return "computer" }
func (module) ReplacesPlugins() []string { return []string{"computer"} }

func (module) Catalog() []moduleapi.ToolMeta {
	catalog, err := loadCatalog()
	if err != nil {
		return nil
	}
	metas := make([]moduleapi.ToolMeta, 0, len(catalog.Tools))
	for _, spec := range catalog.Tools {
		metas = append(metas, spec.Meta())
	}
	return metas
}

func (module) ToolsForTurn(ctx context.Context, host moduleapi.Host, inv *store.AgentInvocation) ([]engine.Tool, error) {
	if installed, _, err := host.PluginInstalled(ctx, "computer"); err != nil {
		return nil, err
	} else if installed {
		if _, captured := host.PluginCaptured(inv, "computer"); !captured {
			return nil, nil
		}
	} else {
		enabled, err := host.ComputerEnabled(ctx)
		if err != nil {
			return nil, err
		}
		if !enabled {
			return nil, nil
		}
	}
	catalog, err := loadCatalog()
	if err != nil {
		return nil, err
	}
	return host.ConnectComputerMCP(ctx, inv, catalog)
}
