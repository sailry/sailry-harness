package node

import (
	"context"

	"github.com/sailry/sailry-harness/internal/node/plugins"
)

func (node *Node) pluginRuntime() plugins.Runtime {
	runtime := plugins.Runtime{ProductID: node.productID}
	if runtime.ProductID == "" {
		runtime.ProductID = "harness"
	}
	running := node.runningModuleServices()
	for _, module := range node.registeredModules {
		if module.RuntimeAvailable(running) {
			runtime.Modules = append(runtime.Modules, module.ID())
		}
	}
	return runtime
}

func (node *Node) pluginContext(ctx context.Context) context.Context {
	return plugins.WithRuntime(ctx, node.pluginRuntime())
}
