// Package ocrnode provides the compile-time native Node OCR model discovery
// and lifecycle agent tools paired with modules/ocr/desktop.
package ocrnode

import (
	"context"
	_ "embed"
	"encoding/json"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

//go:embed tools.json
var catalogJSON []byte

var loadCatalog = sync.OnceValues(func() (moduleapi.CatalogSpec, error) {
	return moduleapi.LoadCatalog(catalogJSON)
})

type module struct{}

// Module returns the native Node module providing OCR model management tools.
func Module() moduleapi.Module { return module{} }

func (module) ID() string                { return "ocr" }
func (module) ReplacesPlugins() []string { return nil }

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

func (module) ToolsForTurn(_ context.Context, host moduleapi.Host, inv *store.AgentInvocation) ([]engine.Tool, error) {
	catalog, err := loadCatalog()
	if err != nil {
		return nil, err
	}
	tools := make([]engine.Tool, 0, len(catalog.Tools))
	for _, spec := range catalog.Tools {
		if !spec.Available(inv) {
			continue
		}
		if spec.Name == "list_ocr_models" {
			spec.ReadOnly = true
			spec.Concurrent = true
			tool, err := moduleapi.BuildCustomTool(spec, "", func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
				var obj map[string]any
				if err := json.Unmarshal(call.Arguments, &obj); err != nil || obj == nil {
					return invalidResult()
				}
				installedOnly := false
				for k, v := range obj {
					if k != "installed_only" {
						return invalidResult()
					}
					b, ok := v.(bool)
					if !ok {
						return invalidResult()
					}
					installedOnly = b
				}
				entries, err := host.ListLocalModels(ctx, protocol.ModelsKindOcr)
				if err != nil {
					return faultResult(err)
				}
				if entries == nil {
					entries = []protocol.ModelsLocalModel{}
				}
				if installedOnly {
					filtered := make([]protocol.ModelsLocalModel, 0, len(entries))
					for _, entry := range entries {
						if entry.State.Kind == "installed" {
							filtered = append(filtered, entry)
						}
					}
					entries = filtered
				}
				return engine.ToolResult{Value: map[string]any{"models": entries}}
			})
			if err != nil {
				return nil, err
			}
			tools = append(tools, tool)
			continue
		}
		tool, err := host.ScopedTool(inv, moduleapi.ScopedToolConfig{
			Spec:        spec,
			PrepareArgs: prepareModelRef,
		})
		if err != nil {
			return nil, err
		}
		tools = append(tools, tool)
	}
	return tools, nil
}

func invalidFault() *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "Invalid OCR model arguments"}
}

func invalidResult() engine.ToolResult {
	return engine.ToolResult{Failure: invalidFault()}
}

func faultResult(err error) engine.ToolResult {
	if fault, ok := err.(*protocol.Fault); ok {
		return engine.ToolResult{Failure: fault}
	}
	return engine.ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: err.Error()}}
}

func isLowerHex64(s string) bool {
	if len(s) != 64 {
		return false
	}
	for i := 0; i < 64; i++ {
		c := s[i]
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f')) {
			return false
		}
	}
	return true
}

func prepareModelRef(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
	var obj map[string]any
	if err := json.Unmarshal(raw, &obj); err != nil || obj == nil || len(obj) != 1 {
		return nil, invalidFault()
	}
	id, ok := obj["id"].(string)
	if !ok || !isLowerHex64(id) {
		return nil, invalidFault()
	}
	return json.Marshal(map[string]any{
		"model": map[string]any{"target": "node", "kind": "ocr", "id": id},
	})
}
