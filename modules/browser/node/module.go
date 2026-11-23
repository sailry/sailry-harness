// Package browsernode provides the compile-time native Node adapters for the
// built-in controller browser and managed external Chromium browser tools.
//
// Built-in browser tool descriptions, schemas and approval groups originate
// from Sailry c4bb60a2 (plugins/builtin/browser/agent.rs). External browser
// tool metadata is adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970
// and preserves Sailry e6f7041a's external-browser adapter semantics.
package browsernode

import (
	"context"
	_ "embed"
	"encoding/json"
	"math"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

//go:embed builtin_tools.json
var builtinCatalogJSON []byte

//go:embed external_tools.json
var externalCatalogJSON []byte

var loadBuiltinCatalog = sync.OnceValues(func() (moduleapi.CatalogSpec, error) {
	return moduleapi.LoadCatalog(builtinCatalogJSON)
})

var loadExternalCatalog = sync.OnceValues(func() (moduleapi.CatalogSpec, error) {
	return moduleapi.LoadCatalog(externalCatalogJSON)
})

var browserCaptions = map[string]protocol.PluginDesktopNavigation{
	"browser tab is empty; navigate to a URL before reading or interacting": {
		Label:   "The tab is empty; open a page first",
		Locales: map[string]string{"zh-CN": "标签页为空，请先打开网页"},
	},
	"browser page is unavailable": {
		Label:   "Page unavailable",
		Locales: map[string]string{"zh-CN": "网页不可用"},
	},
	"browser page failed to load": {
		Label:   "Page failed to load",
		Locales: map[string]string{"zh-CN": "网页加载失败"},
	},
}

type builtinModule struct{}

// Builtin returns the native Node module for the initiating controller's
// built-in browser.
func Builtin() moduleapi.Module { return builtinModule{} }

func (builtinModule) ID() string                { return "browser" }
func (builtinModule) ReplacesPlugins() []string { return []string{"browser"} }

func (builtinModule) Catalog() []moduleapi.ToolMeta {
	catalog, err := loadBuiltinCatalog()
	if err != nil {
		return nil
	}
	metas := make([]moduleapi.ToolMeta, 0, len(catalog.Tools))
	for _, spec := range catalog.Tools {
		metas = append(metas, spec.Meta())
	}
	return metas
}

func (builtinModule) ToolsForTurn(ctx context.Context, host moduleapi.Host, inv *store.AgentInvocation) ([]engine.Tool, error) {
	if !host.BrowserAvailable(inv) {
		return nil, nil
	}
	if installed, _, err := host.PluginInstalled(ctx, "browser"); err != nil {
		return nil, err
	} else if installed {
		if _, captured := host.PluginCaptured(inv, "browser"); !captured {
			return nil, nil
		}
	}
	catalog, err := loadBuiltinCatalog()
	if err != nil {
		return nil, err
	}
	tools := make([]engine.Tool, 0, len(catalog.Tools))
	for _, spec := range catalog.Tools {
		if !spec.Available(inv) {
			continue
		}
		action := spec.Action
		tool, err := host.BrowserTool(inv, moduleapi.BrowserToolConfig{
			Spec:         spec,
			Instruction:  catalog.Instructions,
			PluginGate:   "browser",
			PrepareArgs:  prepareBuiltin(action),
			FormatResult: formatBuiltinResult,
		})
		if err != nil {
			return nil, err
		}
		tools = append(tools, tool)
	}
	return tools, nil
}

func prepareBuiltin(action string) func(context.Context, json.RawMessage) (json.RawMessage, error) {
	return func(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
		var args map[string]any
		dec := json.NewDecoder(strings.NewReader(string(raw)))
		dec.UseNumber()
		if err := dec.Decode(&args); err != nil || args == nil {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "browser arguments must be an object"}
		}
		if action == "open" || action == "navigate" {
			if bg, exists := args["background"]; exists {
				if _, ok := bg.(bool); !ok {
					return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "browser background must be a boolean"}
				}
			} else {
				args["background"] = false
			}
		}
		if action == "wait" {
			cond, _ := args["condition"].(map[string]any)
			kind, _ := cond["kind"].(string)
			query := ""
			switch kind {
			case "text":
				query, _ = cond["text"].(string)
			case "visible", "hidden":
				query, _ = cond["selector"].(string)
			}
			timeout, ok := uint32Positive(args["timeout_ms"])
			if !ok || timeout == 0 || query == "" {
				return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid browser wait condition or timeout"}
			}
		}
		return json.Marshal(map[string]any{"action": action, "arguments": args})
	}
}

func uint32Positive(v any) (uint32, bool) {
	switch n := v.(type) {
	case json.Number:
		if i, err := n.Int64(); err == nil && i >= 1 && i <= 0xffffffff {
			return uint32(i), true
		}
		if f, err := n.Float64(); err == nil && !math.IsNaN(f) && !math.IsInf(f, 0) && math.Trunc(f) == f && f >= 1 && f <= 0xffffffff {
			return uint32(f), true
		}
	case float64:
		if !math.IsNaN(n) && !math.IsInf(n, 0) && math.Trunc(n) == n && n >= 1 && n <= 0xffffffff {
			return uint32(n), true
		}
	}
	return 0, false
}

func formatBuiltinResult(_ context.Context, res engine.ToolResult) engine.ToolResult {
	if res.Failure != nil {
		if msg, ok := browserCaptions[res.Failure.Message]; ok {
			return engine.ToolResult{
				Value: map[string]any{
					"sailry_content": protocol.ToolContent{
						Version: 1,
						Blocks: []protocol.ToolContentBlock{
							{Kind: "notice", Data: map[string]any{"message": msg}},
						},
					},
				},
			}
		}
		return res
	}
	if m, ok := res.Value.(map[string]any); ok {
		if data, exists := m["data"]; exists {
			res.Value = data
		}
	}
	return res
}

type externalModule struct{}

// External returns the native Node module for Sailry's isolated Chromium
// external browser.
func External() moduleapi.Module { return externalModule{} }

func (externalModule) ID() string                { return "external-browser" }
func (externalModule) ReplacesPlugins() []string { return []string{"external-browser"} }

func (externalModule) Catalog() []moduleapi.ToolMeta {
	catalog, err := loadExternalCatalog()
	if err != nil {
		return nil
	}
	metas := make([]moduleapi.ToolMeta, 0, len(catalog.Tools))
	for _, spec := range catalog.Tools {
		metas = append(metas, spec.Meta())
	}
	return metas
}

func (externalModule) ToolsForTurn(ctx context.Context, host moduleapi.Host, inv *store.AgentInvocation) ([]engine.Tool, error) {
	if !host.ExternalBrowserAvailable() {
		return nil, nil
	}
	if installed, _, err := host.PluginInstalled(ctx, "external-browser"); err != nil {
		return nil, err
	} else if installed {
		if _, captured := host.PluginCaptured(inv, "external-browser"); !captured {
			return nil, nil
		}
	} else {
		enabled, err := host.ExternalBrowserEnabled(ctx)
		if err != nil {
			return nil, err
		}
		if !enabled {
			return nil, nil
		}
	}
	catalog, err := loadExternalCatalog()
	if err != nil {
		return nil, err
	}
	closeStopped := host.ExternalBrowserCloseOnCancel(ctx, inv.Session.ID)
	tools := make([]engine.Tool, 0, len(catalog.Tools))
	for _, spec := range catalog.Tools {
		if !spec.Available(inv) {
			continue
		}
		action := spec.Action
		tool, err := host.ExternalBrowserTool(inv, moduleapi.ExternalBrowserToolConfig{
			Spec:         spec,
			Instruction:  catalog.Instructions,
			PluginGate:   "external-browser",
			Close:        closeStopped,
			PrepareArgs:  prepareExternal(action),
			FormatResult: formatExternalResult,
		})
		if err != nil {
			return nil, err
		}
		tools = append(tools, tool)
	}
	return tools, nil
}

func prepareExternal(action string) func(context.Context, json.RawMessage) (json.RawMessage, error) {
	return func(_ context.Context, raw json.RawMessage) (json.RawMessage, error) {
		var args map[string]any
		dec := json.NewDecoder(strings.NewReader(string(raw)))
		dec.UseNumber()
		if err := dec.Decode(&args); err != nil || args == nil {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "Browser arguments must be an object"}
		}
		if action == "wait" {
			secVal, exists := args["seconds"]
			if !exists {
				return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "browser wait seconds must be finite and non-negative"}
			}
			var seconds float64
			switch n := secVal.(type) {
			case json.Number:
				f, err := n.Float64()
				if err != nil {
					return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "browser wait seconds must be finite and non-negative"}
				}
				seconds = f
			case float64:
				seconds = n
			default:
				return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "browser wait seconds must be finite and non-negative"}
			}
			if math.IsNaN(seconds) || math.IsInf(seconds, 0) || seconds < 0 {
				return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "browser wait seconds must be finite and non-negative"}
			}
		}
		return json.Marshal(map[string]any{"action": action, "arguments": json.RawMessage(raw)})
	}
}

func formatExternalResult(_ context.Context, res engine.ToolResult) engine.ToolResult {
	if res.Failure != nil {
		return res
	}
	if m, ok := res.Value.(map[string]any); ok {
		if data, exists := m["data"]; exists {
			res.Value = data
		}
	}
	return res
}
