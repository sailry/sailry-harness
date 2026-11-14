package node

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAssistantToolsAddOrdinaryContext(t *testing.T) {
	for _, resource := range []string{"ssh", "database"} {
		t.Run(resource, func(t *testing.T) {
			invocation := &store.AgentInvocation{}
			invocation.Turn.Config.Resource = &protocol.ConnectionResource{Kind: resource}
			local := operations.Declaration{Contexts: []string{"plugin", "workspace"}}
			scripted := protocol.ToolDeclaration{Contexts: []protocol.PluginConversationResource{protocol.PluginConversationResourcePlugin, protocol.PluginConversationResourceWorkspace}}
			if availableOperationTool(invocation, local) || availablePluginTool(invocation, scripted) {
				t.Fatal("a legacy connection conversation changed its tool context")
			}
			invocation.Turn.Config.Assistant = &protocol.PluginConversationBinding{Package: protocol.PluginReference{Name: "assistant"}, ID: "bound"}
			if !availableOperationTool(invocation, local) || !availablePluginTool(invocation, scripted) || !availableOperationTool(invocation, operations.Declaration{Contexts: []string{resource}}) {
				t.Fatal("the assistant replaced ordinary tools with its specialized context")
			}
			server := "fixture"
			info := protocol.PluginInfo{Summary: protocol.PluginSummary{Name: "another-package"}, Extension: &protocol.PluginExtension{Tools: []protocol.ToolDeclaration{{Name: "read", Server: &server, Contexts: scripted.Contexts}}}}
			if declaration, accepted := selectedMCPTool(invocation, info, server, "read"); !accepted || declaration == nil {
				t.Fatal("the assistant filtered an ordinary MCP tool from another package")
			}
		})
	}
}
