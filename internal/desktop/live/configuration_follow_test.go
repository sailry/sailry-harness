package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConfigurationNodeIDUsesCanonicalHex(t *testing.T) {
	id := protocol.NodeID{1, 2, 3}
	if value, ok := configurationNodeID(id.String()); !ok || value != id {
		t.Fatal("canonical identity was not preserved")
	}
	for _, raw := range []string{"", "abc", "not-an-id"} {
		if _, ok := configurationNodeID(raw); ok {
			t.Fatal("invalid identity was accepted")
		}
	}
}
func TestConfigurationFollowProjectionKeepsPerServerLocalChoice(t *testing.T) {
	r := &Runtime{}
	// No host display lookup is needed when the view has no target/source peers.
	view := &settings.ConfigurationFollowEditor{}
	plugin := protocol.DefaultPluginInfo()
	plugin.Summary.Name = "fixture"
	plugin.Summary.Source = &protocol.ConfigurationOrigin{Node: protocol.NodeID{8}, Revision: 2}
	plugin.Mcp = []protocol.PluginMcp{{Name: "alpha"}, {Name: "beta"}}
	r.projectConfigurationFollow(view, protocol.ConfigurationFollowStatus{Revision: 1, MCPLocal: []protocol.ConfigurationMCPLocal{{Plugin: "fixture", Server: "beta"}}}, protocol.IdentityState{}, []protocol.PluginInfo{plugin})
	if !view.Disabled || len(view.MCP) != 2 || view.MCP[0].Local || !view.MCP[1].Local {
		t.Fatalf("local server choices leaked across servers: %+v", view.MCP)
	}
}
