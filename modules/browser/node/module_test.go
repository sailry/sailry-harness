package browsernode

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type browserToolHost struct {
	moduleapi.Host
	available, enabled bool
	builtin            map[string]moduleapi.BrowserToolConfig
	external           map[string]moduleapi.ExternalBrowserToolConfig
}

func (h browserToolHost) BrowserAvailable(*store.AgentInvocation) bool { return h.available }
func (h browserToolHost) ExternalBrowserAvailable() bool               { return h.available }
func (h browserToolHost) PluginInstalled(context.Context, string) (bool, bool, error) {
	return false, false, nil
}
func (h browserToolHost) ExternalBrowserEnabled(context.Context) (bool, error) {
	return h.enabled, nil
}
func (browserToolHost) ExternalBrowserCloseOnCancel(context.Context, protocol.SessionID) func() error {
	return func() error { return nil }
}
func (h browserToolHost) BrowserTool(_ *store.AgentInvocation, cfg moduleapi.BrowserToolConfig) (engine.Tool, error) {
	h.builtin[cfg.Spec.Name] = cfg
	return engine.Tool{}, nil
}
func (h browserToolHost) ExternalBrowserTool(_ *store.AgentInvocation, cfg moduleapi.ExternalBrowserToolConfig) (engine.Tool, error) {
	h.external[cfg.Spec.Name] = cfg
	return engine.Tool{}, nil
}

func TestBrowserModuleAdapters(t *testing.T) {
	inv := &store.AgentInvocation{}
	host := browserToolHost{
		builtin:  make(map[string]moduleapi.BrowserToolConfig),
		external: make(map[string]moduleapi.ExternalBrowserToolConfig),
	}
	if tools, err := Builtin().ToolsForTurn(t.Context(), host, inv); err != nil || len(tools) != 0 {
		t.Fatalf("controller tools without a browser = %d, %v", len(tools), err)
	}
	host.available = true
	if tools, err := Builtin().ToolsForTurn(t.Context(), host, inv); err != nil || len(tools) == 0 {
		t.Fatalf("controller tools = %d, %v", len(tools), err)
	}
	navigate := host.builtin["browser_navigate"]
	if navigate.PluginGate != "browser" || navigate.Spec.LegacyAlias() == "" {
		t.Fatal("browser tool lost its historical gate or alias")
	}
	if args, err := navigate.PrepareArgs(t.Context(), json.RawMessage(`{"url":"https://example.test/"}`)); err != nil || string(args) != `{"action":"navigate","arguments":{"background":false,"url":"https://example.test/"}}` {
		t.Fatalf("prepared browser navigation = %s, %v", args, err)
	}
	if tools, err := External().ToolsForTurn(t.Context(), host, inv); err != nil || len(tools) != 0 {
		t.Fatalf("disabled external browser tools = %d, %v", len(tools), err)
	}
	host.enabled = true
	if tools, err := External().ToolsForTurn(t.Context(), host, inv); err != nil || len(tools) == 0 {
		t.Fatalf("external browser tools = %d, %v", len(tools), err)
	}
	wait := host.external["external_browser_wait"]
	if wait.Close == nil || wait.PrepareArgs == nil || wait.Spec.LegacyAlias() == "" {
		t.Fatal("external browser tool lost its cancel lifecycle or legacy alias")
	}
	if _, err := wait.PrepareArgs(t.Context(), json.RawMessage(`{"seconds":-1}`)); err == nil {
		t.Fatal("external browser wait accepted a negative duration")
	}
	if args, err := wait.PrepareArgs(t.Context(), json.RawMessage(`{"seconds":0}`)); err != nil || string(args) != `{"action":"wait","arguments":{"seconds":0}}` {
		t.Fatalf("prepared external browser wait = %s, %v", args, err)
	}
}
