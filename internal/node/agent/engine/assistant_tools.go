package engine

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// assistantBuiltins are the native tools an assistant declaration may name.
var assistantBuiltins = []string{
	"ask_user", "set_session_title",
	"list_sessions", "read_session", "search_session", "read_tool_output",
	"load_skill", "read_skill_resource", "manage_skills", "manage_mcp", "manage_plugins",
	"spawn_agent", "guide_agent", "wait_agent",
	"list_agents", "add_agent", "edit_agent", "delete_agent", "apply_agents",
	"list_teams", "add_team", "edit_team", "delete_team", "apply_team",
}

// applyAssistant narrows the captured tools to the frozen assistant declaration
// and adds its context. Every remaining tool is offered on the first request.
func (r *Runtime) applyAssistant(x *execution) error {
	binding := x.inv.Turn.Config.Assistant
	if binding == nil {
		return nil
	}
	declaration := x.inv.Assistant
	if declaration == nil {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "plugin assistant declaration is unavailable"}
	}
	allowed := map[string]bool{}
	for _, tool := range declaration.Tools {
		switch value := tool.Data.(type) {
		case *protocol.PluginConversationToolBuiltinPayload:
			if !slices.Contains(assistantBuiltins, value.Name) {
				return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "plugin assistant references an unknown native tool"}
			}
			allowed[value.Name] = true
		case *protocol.PluginConversationToolPluginPayload:
			if value.Server != nil {
				allowed[mcp.Alias(binding.Package.Name, *value.Server, value.Name)] = true
			} else {
				allowed[operations.Alias(binding.Package.Name, value.Name)] = true
			}
		case *protocol.PluginConversationToolPackagePayload:
			allowed[operations.Alias(value.Package, value.Name)] = true
		}
	}
	if selectedRole(x.inv.Message) != nil && !slices.ContainsFunc(x.tools, func(tool Tool) bool {
		return tool.Delegation && allowed[tool.Definition.Name]
	}) {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin assistant does not allow delegation"}
	}
	x.tools = slices.DeleteFunc(x.tools, func(tool Tool) bool {
		return tool.Definition.Name != "read_tool_output" && !allowed[tool.Definition.Name]
	})
	for index := range x.tools {
		x.tools[index].Deferred = false
	}
	x.assistantContext = declaration.Context
	return nil
}
