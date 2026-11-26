package node

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
)

// scopedTools binds the declared local model and role catalog operations of a
// turn. Each call is dispatched under the declaring package, so the store
// checks its declared software action, and a model removal is admitted only as
// the approved call that captured it. Handler tools wrap these bindings with
// their script preparation.
func (node *Node) scopedTools(inv *store.AgentInvocation, catalog *operations.Catalog) ([]engine.Tool, error) {
	host := nodeModuleHost{node: node}
	var tools []engine.Tool
	for _, declaration := range catalog.Tools {
		if node.hasReplacedPlugin(declaration.Owner.Name, declaration.Owner.Digest) {
			continue
		}
		if !operations.ScopedOperation(declaration.Operation) || !slices.Contains(inv.Turn.Plugins, declaration.Owner) ||
			!availableOperationTool(inv, declaration) ||
			inv.Turn.Config.Resource != nil && inv.Turn.Config.Assistant == nil && len(declaration.Contexts) == 0 {
			continue
		}
		tool, err := host.buildScopedTool(inv, declaration.Owner, moduleapi.ScopedToolConfig{
			Spec: moduleapi.ToolSpec{
				Name:        declaration.Name,
				Description: declaration.Description,
				Operation:   declaration.Operation,
				Parameters:  declaration.Parameters,
				Initial:     declaration.Initial,
			},
		})
		if err != nil {
			return nil, err
		}
		tools = append(tools, tool)
	}
	return tools, nil
}
