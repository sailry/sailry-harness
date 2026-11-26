package node

import (
	"context"

	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
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

func (node *Node) nativeDispatchNamespaces() []string {
	var namespaces []string
	for _, module := range node.registeredModules {
		if provider, ok := module.Module.(moduleapi.DispatchProvider); ok {
			if name := provider.NativeDispatchNamespace(); name != "" {
				namespaces = append(namespaces, name)
			}
		}
	}
	return namespaces
}

func (node *Node) pluginContext(ctx context.Context) context.Context {
	ctx = plugins.WithRuntime(ctx, node.pluginRuntime())
	return store.WithNativeDispatch(ctx, node.nativeDispatchNamespaces())
}
