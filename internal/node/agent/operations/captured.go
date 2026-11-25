package operations

import (
	"context"
	"runtime"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func CaptureCatalog(ctx context.Context, profile string, references []protocol.PluginReference) (*Catalog, error) {
	specifications, err := operationPolicies()
	if err != nil {
		return nil, err
	}
	manager := plugins.NewManager(profile)
	result := &Catalog{References: slices.Clone(references)}
	for _, reference := range references {
		info, err := manager.CapturedInfo(ctx, reference)
		if err != nil {
			return nil, err
		}
		if info.Extension == nil {
			continue
		}
		for _, tool := range info.Extension.Tools {
			operation := tool.Operation
			if tool.Handler != nil && tool.Handler.Operation != nil {
				operation = tool.Handler.Operation
			}
			if operation == nil || len(tool.Platforms) != 0 && !slices.Contains(tool.Platforms, runtime.GOOS) {
				continue
			}
			spec, exists := specifications[*operation]
			if !exists {
				continue
			}
			if description := tool.PlatformDescriptions[runtime.GOOS]; description != "" {
				spec.Description = description
			} else if tool.Description != nil {
				spec.Description = *tool.Description
			}
			result.Tools = append(result.Tools, Declaration{Spec: spec, Owner: reference, Name: Alias(reference.Name, tool.Name), Exposed: tool.Name, Initial: tool.Initial, Script: info.Extension.Host != nil && tool.Handler != nil, Contexts: tool.Contexts, Presentation: tool.Presentation, Grouping: tool.Grouping, Display: tool.Display})
		}
	}
	return result, nil
}
