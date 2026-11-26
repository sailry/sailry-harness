package engine

import (
	"encoding/json"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/santhosh-tekuri/jsonschema/v6"
)

type inheritedTool struct {
	binding    *store.ApprovalBinding
	authority  mcp.AuthorityKey
	schema     *jsonschema.Schema
	schemaJSON json.RawMessage
	flow       []protocol.ToolOperation
	readOnly   bool
	approval   bool
	confirm    bool
	deferred   bool
}

type storedInheritedTool struct {
	Binding    *store.ApprovalBinding   `json:"binding,omitempty"`
	Authority  mcp.AuthorityKey         `json:"authority,omitempty"`
	SchemaJSON json.RawMessage          `json:"schema_json,omitempty"`
	Flow       []protocol.ToolOperation `json:"flow,omitempty"`
	ReadOnly   bool                     `json:"read_only,omitempty"`
	Approval   bool                     `json:"approval,omitempty"`
	Confirm    bool                     `json:"confirm,omitempty"`
	Deferred   bool                     `json:"deferred,omitempty"`
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
			rawSchema := tool.SchemaJSON
			if len(rawSchema) == 0 && tool.Definition.InputSchema != nil {
				if encoded, err := json.Marshal(tool.Definition.InputSchema); err == nil {
					rawSchema = encoded
				}
			}
			allowed[name] = inheritedTool{
				binding: tool.Binding, authority: tool.MCPAuthority, schema: tool.schema, schemaJSON: rawSchema, flow: tool.Flow,
				readOnly: tool.ReadOnly, approval: tool.Approval, confirm: tool.Confirm != nil, deferred: tool.Deferred,
			}
		}
	}
	return allowed
}

func encodeInheritedTools(tools inheritedTools) ([]byte, error) {
	if len(tools) == 0 {
		return nil, nil
	}
	stored := make(map[string]storedInheritedTool, len(tools))
	for name, tool := range tools {
		stored[name] = storedInheritedTool{
			Binding:    tool.binding,
			Authority:  tool.authority,
			SchemaJSON: tool.schemaJSON,
			Flow:       tool.flow,
			ReadOnly:   tool.readOnly,
			Approval:   tool.approval,
			Confirm:    tool.confirm,
			Deferred:   tool.deferred,
		}
	}
	return json.Marshal(stored)
}

func decodeInheritedTools(raw []byte) (inheritedTools, error) {
	if len(raw) == 0 {
		return nil, nil
	}
	var stored map[string]storedInheritedTool
	if err := json.Unmarshal(raw, &stored); err != nil {
		return nil, err
	}
	allowed := make(inheritedTools, len(stored))
	for name, tool := range stored {
		var compiled *jsonschema.Schema
		if len(tool.SchemaJSON) > 0 {
			var parsed any
			if err := json.Unmarshal(tool.SchemaJSON, &parsed); err != nil {
				return nil, err
			}
			var err error
			compiled, err = compileToolSchema(name, tool.SchemaJSON, parsed)
			if err != nil {
				return nil, err
			}
		}
		allowed[name] = inheritedTool{
			binding:    tool.Binding,
			authority:  tool.Authority,
			schema:     compiled,
			schemaJSON: tool.SchemaJSON,
			flow:       tool.Flow,
			readOnly:   tool.ReadOnly,
			approval:   tool.Approval,
			confirm:    tool.Confirm,
			deferred:   tool.Deferred,
		}
	}
	return allowed, nil
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
		case "spawn_agent", "guide_agent", "wait_agent", "list_subagents", "stop_agent", "set_session_title", "ask_user":
			return true
		}
		if slices.Contains(catalogToolNames, tool.Definition.Name) {
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
