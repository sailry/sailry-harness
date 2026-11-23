package node

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAssistantToolsAddOrdinaryContext(t *testing.T) {
	t.Parallel()
	for _, resource := range []protocol.PluginConversationResource{protocol.PluginConversationResourceSsh, protocol.PluginConversationResourceDatabase} {
		t.Run(string(resource), func(t *testing.T) {
			invocation := &store.AgentInvocation{}
			invocation.Turn.Config.Resource = &protocol.ConnectionResource{Kind: string(resource)}
			local := operations.Declaration{Contexts: []protocol.PluginConversationResource{protocol.PluginConversationResourcePlugin}}
			scripted := protocol.ToolDeclaration{Contexts: local.Contexts}
			if availableOperationTool(invocation, local) || availablePluginTool(invocation, scripted) {
				t.Fatal("a legacy connection conversation changed its tool context")
			}
			invocation.Turn.Config.Assistant = &protocol.PluginConversationBinding{Package: protocol.PluginReference{Name: "assistant"}, ID: "bound"}
			if !availableOperationTool(invocation, local) || !availablePluginTool(invocation, scripted) || !availableOperationTool(invocation, operations.Declaration{Contexts: []protocol.PluginConversationResource{resource}}) {
				t.Fatal("the assistant replaced ordinary tools with its specialized context")
			}
			workspace := []protocol.PluginConversationResource{protocol.PluginConversationResourceWorkspace}
			if availableOperationTool(invocation, operations.Declaration{Contexts: workspace}) || availablePluginTool(invocation, protocol.ToolDeclaration{Contexts: workspace}) {
				t.Fatal("a bound assistant gained the unbound workspace context")
			}
			server := "fixture"
			info := protocol.PluginInfo{Summary: protocol.PluginSummary{Name: "another-package"}, Extension: &protocol.PluginExtension{Tools: []protocol.ToolDeclaration{{Name: "read", Server: &server, Contexts: scripted.Contexts}}}}
			if declaration, accepted := selectedMCPTool(invocation, info, server, "read"); !accepted || declaration == nil {
				t.Fatal("the assistant filtered an ordinary MCP tool from another package")
			}
		})
	}
}

func TestConversationToolContexts(t *testing.T) {
	t.Parallel()
	project := protocol.NewProjectID()
	assistant := &protocol.PluginConversationBinding{Package: protocol.PluginReference{Name: "assistant"}, ID: "fixture"}
	database := &protocol.ConnectionResource{Kind: "database"}
	ssh := &protocol.ConnectionResource{Kind: "ssh"}
	for _, entry := range []struct {
		name      string
		project   *protocol.ProjectID
		config    protocol.SessionConfig
		available []protocol.PluginConversationResource
	}{
		{"ordinary", nil, protocol.SessionConfig{}, []protocol.PluginConversationResource{"workspace", "plugin"}},
		{"project", &project, protocol.SessionConfig{}, []protocol.PluginConversationResource{"workspace"}},
		{"plugin assistant", nil, protocol.SessionConfig{Assistant: assistant}, []protocol.PluginConversationResource{"plugin"}},
		{"bound database", nil, protocol.SessionConfig{Resource: database}, []protocol.PluginConversationResource{"database"}},
		{"bound SSH", nil, protocol.SessionConfig{Resource: ssh}, []protocol.PluginConversationResource{"ssh"}},
	} {
		t.Run(entry.name, func(t *testing.T) {
			invocation := &store.AgentInvocation{}
			invocation.Worktree.Project, invocation.Turn.Config = entry.project, entry.config
			server := "fixture"
			for _, resource := range []protocol.PluginConversationResource{"workspace", "plugin", "database", "ssh", ""} {
				var contexts []protocol.PluginConversationResource
				want := resource == ""
				if resource != "" {
					contexts = []protocol.PluginConversationResource{resource}
					for _, allowed := range entry.available {
						want = want || resource == allowed
					}
				}
				tool := protocol.ToolDeclaration{Name: "read", Server: &server, Contexts: contexts}
				info := protocol.PluginInfo{Extension: &protocol.PluginExtension{Tools: []protocol.ToolDeclaration{tool}}}
				_, mcpAvailable := selectedMCPTool(invocation, info, server, tool.Name)
				if native, scripted := availableOperationTool(invocation, operations.Declaration{Contexts: contexts}), availablePluginTool(invocation, tool); native != want || scripted != want || mcpAvailable != want {
					t.Errorf("context %q: native=%v scripted=%v MCP=%v, want %v", resource, native, scripted, mcpAvailable, want)
				}
			}
		})
	}
}
