package plugins

import (
	"context"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Runtime identifies the execution product and the modules it actually assembled.
// An unset runtime leaves controller-local version checks independent of the Node.
type Runtime struct {
	ProductID string
	Modules   []string
}

type runtimeKey struct{}

func WithRuntime(ctx context.Context, runtime Runtime) context.Context {
	runtime.Modules = slices.Clone(runtime.Modules)
	return context.WithValue(ctx, runtimeKey{}, runtime)
}

func RuntimeFromContext(ctx context.Context) Runtime {
	runtime, _ := ctx.Value(runtimeKey{}).(Runtime)
	runtime.Modules = slices.Clone(runtime.Modules)
	return runtime
}

func refreshCompatibility(info *protocol.PluginInfo, runtime Runtime) {
	info.Summary.Compatibility = InfoCompatibility(*info, runtime)
	for index := range info.Versions {
		info.Versions[index].Compatibility = CatalogVersionCompatibility(info.Versions[index], runtime)
	}
}

func validRuntimeRequirements(products, modules []string) bool {
	if products != nil && len(products) == 0 {
		return false
	}
	for _, values := range [][]string{products, modules} {
		seen := map[string]bool{}
		for _, id := range values {
			if !ValidateName(id) || seen[id] {
				return false
			}
			seen[id] = true
		}
	}
	return true
}

func runtimeCompatibility(products, modules []string, runtimes []Runtime) protocol.PluginCompatibility {
	if !validRuntimeRequirements(products, modules) {
		return unsupportedCompatibility("invalid plugin product or module requirements")
	}
	for _, runtime := range runtimes {
		if runtime.ProductID == "" && runtime.Modules == nil {
			continue
		}
		if !ValidateName(runtime.ProductID) {
			return unsupportedCompatibility("Sailry Node product is invalid")
		}
		if len(products) != 0 && !slices.Contains(products, runtime.ProductID) {
			return unsupportedCompatibility("plugin does not support product " + runtime.ProductID)
		}
		for _, id := range modules {
			if !slices.Contains(runtime.Modules, id) {
				return unsupportedCompatibility("plugin requires Node module " + id)
			}
		}
	}
	return protocol.PluginCompatibility{Kind: "compatible"}
}
