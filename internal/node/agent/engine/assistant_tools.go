package engine

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// assistantBuiltins are the native tools an assistant declaration may name.
var assistantBuiltins = []string{
	"ask_user", "set_session_title", operations.UpdatePlanTool,
	"list_sessions", "read_session", "search_session", "read_tool_output",
	"load_skill", "read_skill_resource", "manage_skills", "manage_mcp", "manage_plugins",
	"spawn_agent", "guide_agent", "wait_agent", "list_subagents",
	"list_agents", "add_agent", "edit_agent", "delete_agent", "apply_agents",
	"list_teams", "add_team", "edit_team", "delete_team", "apply_team",
	"inspect_image", "generate_image", "generate_video",
}

// applyAssistant adds the frozen assistant's context and offers its preferred
// tools on the first request. Other captured tools keep normal discovery.
func (r *Runtime) applyAssistant(x *execution) error {
	binding := x.inv.Turn.Config.Assistant
	if binding == nil {
		return nil
	}
	declaration := x.inv.Assistant
	if declaration == nil {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "plugin assistant declaration is unavailable"}
	}
	initial := map[string]bool{}
	for _, tool := range declaration.Tools {
		switch value := tool.Data.(type) {
		case *protocol.PluginConversationToolBuiltinPayload:
			if !slices.Contains(assistantBuiltins, value.Name) && !operations.IsCommandTool(value.Name) {
				return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "plugin assistant references an unknown native tool"}
			}
			initial[value.Name] = true
		case *protocol.PluginConversationToolPluginPayload:
			if value.Server != nil {
				initial[mcp.Alias(binding.Package.Name, *value.Server, value.Name)] = true
			} else {
				initial[operations.Alias(binding.Package.Name, value.Name)] = true
			}
		case *protocol.PluginConversationToolPackagePayload:
			name := operations.Alias(value.Package, value.Name)
			if core := operations.LegacyCommandTool(name); core != "" {
				name = core
			}
			initial[name] = true
		}
	}
	for index := range x.tools {
		if initial[x.tools[index].Definition.Name] {
			x.tools[index].Deferred = false
		}
	}
	x.assistantContext = declaration.Context + "\n\nAssistant runtime capabilities\nAssistant tool selectors prioritize initial tools; they do not restrict the ordinary conversation catalog or skills. A bound resource is the default target, and runtime checks determine which targets and operations the captured mode, caller and permissions allow."
	return nil
}
