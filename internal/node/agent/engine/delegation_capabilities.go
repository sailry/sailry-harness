package engine

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/santhosh-tekuri/jsonschema/v6"
)

type inheritedTool struct {
	binding   *store.ApprovalBinding
	authority mcp.AuthorityKey
	schema    *jsonschema.Schema
	flow      []protocol.ToolOperation
	readOnly  bool
	approval  bool
	confirm   bool
	deferred  bool
}

// Only the declarations are shared. A child captures its own executors,
// approval handlers and resources for its turn and worktree.
type inheritedTools map[string]inheritedTool

func captureInheritedTools(tools []Tool) inheritedTools {
	allowed := make(inheritedTools, len(tools))
	for _, tool := range tools {
		if childSchedulingTool(tool) || tool.Definition.Name == "tool_search" || tool.Definition.Name == callToolName {
			continue
		}
		name := tool.Definition.Name
		if _, exists := allowed[name]; !exists {
			allowed[name] = inheritedTool{
				binding: tool.Binding, authority: tool.MCPAuthority, schema: tool.schema, flow: tool.Flow,
				readOnly: tool.ReadOnly, approval: tool.Approval, confirm: tool.Confirm != nil, deferred: tool.Deferred,
			}
		}
	}
	return allowed
}

func (allowed inheritedTools) restrict(tools []Tool) []Tool {
	tools = slices.DeleteFunc(tools, func(tool Tool) bool {
		parent, exists := allowed[tool.Definition.Name]
		if !exists || childSchedulingTool(tool) {
			return true
		}
		if parent.authority != tool.MCPAuthority || !sameToolBinding(parent.binding, tool.Binding) || !slices.Equal(parent.flow, tool.Flow) {
			return true
		}
		return parent.readOnly && !tool.ReadOnly || parent.approval && !tool.Approval ||
			parent.confirm && !tool.Approval && tool.Confirm == nil
	})
	for i := range tools {
		tools[i].parentSchema = allowed[tools[i].Definition.Name].schema
	}
	return tools
}

func sameToolBinding(parent, child *store.ApprovalBinding) bool {
	if parent == nil || child == nil {
		return parent == child
	}
	if parent.Owner != child.Owner || parent.Tool != child.Tool {
		return false
	}
	if parent.Operation == nil || child.Operation == nil {
		return parent.Operation == child.Operation
	}
	return *parent.Operation == *child.Operation
}

func childSchedulingTool(tool Tool) bool {
	if tool.Delegation || tool.BindDelegation != nil {
		return true
	}
	if tool.Binding == nil {
		switch tool.Definition.Name {
		case "spawn_agent", "guide_agent", "wait_agent", "list_subagents", "stop_agent":
			return true
		}
	}
	return slices.ContainsFunc(declaredOperations(tool), func(operation protocol.ToolOperation) bool {
		switch operation {
		case protocol.ToolOperationDelegateAgent, protocol.ToolOperationGuideAgent, protocol.ToolOperationWaitAgent:
			return true
		default:
			return false
		}
	})
}
