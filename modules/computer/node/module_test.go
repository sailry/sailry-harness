package computernode

import (
	"context"
	"slices"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type computerTestHost struct {
	moduleapi.Host
	installed, captured, enabled bool
	catalog                      moduleapi.CatalogSpec
}

func (h *computerTestHost) PluginInstalled(context.Context, string) (bool, bool, error) {
	return h.installed, h.captured, nil
}

func (h *computerTestHost) PluginCaptured(*store.AgentInvocation, string) (protocol.PluginReference, bool) {
	return protocol.PluginReference{}, h.captured
}

func (h *computerTestHost) ComputerEnabled(context.Context) (bool, error) { return h.enabled, nil }

func (h *computerTestHost) ConnectComputerMCP(_ context.Context, _ *store.AgentInvocation, catalog moduleapi.CatalogSpec) ([]engine.Tool, error) {
	h.catalog = catalog
	return []engine.Tool{{Definition: fantasy.FunctionTool{Name: "desktop_snapshot"}}}, nil
}

func TestComputerToolsFollowSelectedFeature(t *testing.T) {
	for _, row := range []struct {
		name                         string
		installed, captured, enabled bool
		want                         bool
	}{
		{name: "disabled by default"},
		{name: "enabled natively", enabled: true, want: true},
		{name: "uncaptured legacy package", installed: true, enabled: true},
		{name: "captured legacy package", installed: true, captured: true, want: true},
	} {
		t.Run(row.name, func(t *testing.T) {
			host := &computerTestHost{installed: row.installed, captured: row.captured, enabled: row.enabled}
			tools, err := Module().ToolsForTurn(t.Context(), host, &store.AgentInvocation{})
			if err != nil || (len(tools) != 0) != row.want {
				t.Fatalf("bound %d tools, error %v", len(tools), err)
			}
			if row.want && (host.catalog.Instructions == "" || !slices.ContainsFunc(host.catalog.Tools, func(spec moduleapi.ToolSpec) bool {
				return spec.Name == "desktop_snapshot" && spec.LegacyPackage == "computer" && spec.LegacyServer == "desktop"
			})) {
				t.Fatal("computer helper did not receive its catalog and historical binding")
			}
		})
	}
}
