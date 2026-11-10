package operations

import (
	"context"
	"encoding/json"
	"runtime"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func CaptureCatalog(ctx context.Context, profile string, references []protocol.PluginReference) (*Catalog, error) {
	bundled, err := Bundled()
	if err != nil {
		return nil, err
	}
	var specifications []Spec
	if err := json.Unmarshal(specification, &specifications); err != nil {
		return nil, err
	}
	manager := plugins.NewManager(profile)
	result := &Catalog{References: slices.Clone(references)}
	for _, reference := range references {
		if slices.ContainsFunc(bundled.References, func(candidate protocol.PluginReference) bool {
			return candidate.Name == reference.Name && candidate.Digest == reference.Digest
		}) {
			for _, declaration := range bundled.Tools {
				if declaration.Owner.Name == reference.Name {
					declaration.Owner = reference
					result.Tools = append(result.Tools, declaration)
				}
			}
			continue
		}
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
			index := slices.IndexFunc(specifications, func(value Spec) bool { return value.Operation == *operation })
			if index < 0 {
				continue
			}
			spec := specifications[index]
			if description := tool.PlatformDescriptions[runtime.GOOS]; description != "" {
				spec.Description = description
			} else if tool.Description != nil {
				spec.Description = *tool.Description
			}
			contexts := make([]string, len(tool.Contexts))
			for index, context := range tool.Contexts {
				contexts[index] = string(context)
			}
			result.Tools = append(result.Tools, Declaration{Spec: spec, Owner: reference, Name: Alias(reference.Name, tool.Name), Exposed: tool.Name, Initial: tool.Initial, Script: info.Extension.Host != nil && tool.Handler != nil, Contexts: contexts, Presentation: tool.Presentation, Grouping: tool.Grouping, Display: tool.Display})
		}
	}
	return result, nil
}
