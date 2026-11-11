package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/python"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) captureResources(x *execution) error {
	var err error
	x.resources, err = operations.CaptureResources(x.ctx, r.profile, x.inv.Turn.Plugins, x.inv.Turn.Config.Assistant)
	if err != nil {
		return err
	}
	x.skillContext, err = x.resources.Catalog()
	if err != nil {
		return err
	}
	mode, err := x.resources.SelectedMode(x.inv.Turn.Config)
	if err != nil {
		return err
	}
	if mode != nil && x.inv.Session.Delegation == nil {
		x.modeContext = mode.Declaration.Instructions
	}
	err = r.db.Read(x.ctx, func(db *sql.Conn) error {
		var err error
		x.childRole, err = store.ReadChildRole(x.ctx, db, x.inv.Run.Turn)
		if err != nil {
			return err
		}
		settings, err := store.ReadTeamSettings(x.ctx, db)
		x.childBudget = settings.ChildConcurrency
		return err
	})
	if err != nil {
		return err
	}
	if x.childRole != nil {
		x.rolePolicy = x.childRole.Instructions
		for _, key := range x.childRole.Skills {
			text, err := x.resources.Read(x.ctx, key, "SKILL.md")
			if err != nil {
				return err
			}
			x.rolePolicy += "\n\nSelected skill " + key + "\nDirectory: " + text.Directory + "\n" + text.Content
		}
	}
	return nil
}

func (r *Runtime) skillTools(x *execution) []Tool {
	if x.resources == nil || len(x.resources.Skills()) == 0 || x.inv.Turn.Config.Resource != nil {
		return nil
	}
	keys := []string{}
	for key := range x.resources.Skills() {
		keys = append(keys, key)
	}
	slices.Sort(keys)
	enum, _ := json.Marshal(keys)
	loadSchema := `{"type":"object","additionalProperties":false,"required":["skill"],"properties":{"skill":{"type":"string","enum":` + string(enum) + `}}}`
	readSchema := `{"type":"object","additionalProperties":false,"required":["skill","path"],"properties":{"skill":{"type":"string"},"path":{"type":"string"}}}`
	var tools []Tool
	for _, resource := range []bool{false, true} {
		name, description, schema := "load_skill", "Load the complete SKILL.md for an available package:skill. The execution Node prepares required Python dependencies and returns python beside directory when available, otherwise null. Run Python scripts with \"$SAILRY_PYTHON\" and absolute script paths; the most recently loaded skill with an available Python interpreter selects it for this turn. Read referenced documents with read_skill_resource. Skill loading grants no command permission.", loadSchema
		if resource {
			name, description, schema = "read_skill_resource", "Read complete UTF-8 text from a file relative to an available skill's directory. Use package:skill and a normalized relative path. Binary or oversized resources report an error, not a partial file.", readSchema
		}
		tools = append(tools, jsonTool(name, description, schema, true, resource, func(ctx context.Context, call canonical.ToolRequest) ToolResult {
			if ctx.Err() != nil || x.ctx.Err() != nil {
				return invalidTool("skill invocation is no longer active")
			}
			var args struct {
				Skill string `json:"skill"`
				Path  string `json:"path"`
			}
			if json.Unmarshal(call.Arguments, &args) != nil {
				return invalidTool("invalid skill arguments")
			}
			if !resource {
				args.Path = "SKILL.md"
			}
			value, err := x.resources.Read(ctx, args.Skill, args.Path)
			if err != nil {
				return toolFault(err)
			}
			out := map[string]any{"skill": args.Skill, "path": args.Path, "content": value.Content, "directory": value.Directory}
			if !resource {
				out["python"] = nil
				ready, err := x.resources.Python(ctx, args.Skill, r.python)
				if err != nil {
					return toolFault(err)
				}
				if ready != nil {
					r.mu.Lock()
					x.python = append(x.python, python.Access{Ready: ready, Directory: value.Directory})
					x.toolCleanup = append(x.toolCleanup, ready.Close)
					r.mu.Unlock()
					out["python"] = ready.Python
				}
			}
			return ToolResult{Value: out}
		}))
	}
	return tools
}

func (r *Runtime) CommandPython(ctx context.Context, turn protocol.TurnID) (*python.Execution, error) {
	if r.python == nil {
		return nil, nil
	}
	r.mu.Lock()
	owner := r.active[turn]
	if owner == nil || owner.ctx.Err() != nil {
		r.mu.Unlock()
		return nil, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "turn is no longer running"}
	}
	skills := slices.Clone(owner.python)
	session := owner.inv.Session.ID
	r.mu.Unlock()
	value, err := r.python.Execution(ctx, session, skills)
	if err != nil && len(skills) == 0 && ctx.Err() == nil {
		return nil, nil
	}
	return value, err
}

func roleToolMatches(selection protocol.PluginConversationTool, tool Tool) bool {
	raw, err := json.Marshal(selection.Data)
	if err != nil {
		return false
	}
	switch selection.Kind {
	case "builtin":
		var value protocol.PluginConversationToolBuiltinPayload
		return json.Unmarshal(raw, &value) == nil && (tool.Binding == nil || tool.Binding.Owner.Name == "") && tool.Definition.Name == value.Name
	case "package":
		var value protocol.PluginConversationToolPackagePayload
		return json.Unmarshal(raw, &value) == nil && tool.Binding != nil && tool.Binding.Owner.Name == value.Package && tool.Definition.Name == operations.Alias(value.Package, value.Name)
	}
	return false
}

// roleAllowsTool selects a child's tools. A role's tool list is exact. Without
// one the child keeps the standard catalog: the reference and skill tools,
// package tools whose every declared operation is standard, and hosted script
// tools that are read-only.
func roleAllowsTool(role *protocol.RoleProfile, tool Tool) bool {
	if tool.Definition.Name == "read_tool_output" || tool.Definition.Name == "tool_search" {
		return true
	}
	if role != nil && role.Tools != nil {
		return slices.ContainsFunc(*role.Tools, func(selection protocol.PluginConversationTool) bool { return roleToolMatches(selection, tool) })
	}
	if tool.Binding == nil {
		return slices.Contains([]string{"list_sessions", "read_session", "search_session", "load_skill", "read_skill_resource"}, tool.Definition.Name)
	}
	if tool.Hosted {
		return tool.ReadOnly
	}
	declared := tool.Flow
	if tool.Binding.Operation != nil {
		declared = append([]protocol.ToolOperation{*tool.Binding.Operation}, declared...)
	}
	return len(declared) != 0 && !slices.ContainsFunc(declared, func(operation protocol.ToolOperation) bool { return !standardOperation(operation) })
}

// standardOperation reports whether a child without a tool list may run the
// operation.
func standardOperation(operation protocol.ToolOperation) bool {
	switch operation {
	case protocol.ToolOperationReadFile, protocol.ToolOperationReadFileLines, protocol.ToolOperationListDirectory, protocol.ToolOperationFindFiles, protocol.ToolOperationSearchFiles, protocol.ToolOperationWriteFile, protocol.ToolOperationEditFile, protocol.ToolOperationApplyChanges, protocol.ToolOperationSearchCode, protocol.ToolOperationOutlineCode, protocol.ToolOperationGraphCode, protocol.ToolOperationInspectGit, protocol.ToolOperationReadGitDiff, protocol.ToolOperationRunCommand, protocol.ToolOperationReadCommand, protocol.ToolOperationStopCommand, protocol.ToolOperationFetchWeb, protocol.ToolOperationSearchWeb, protocol.ToolOperationDeleteFile, protocol.ToolOperationMoveFile, protocol.ToolOperationCreateDirectory, protocol.ToolOperationReadOffice, protocol.ToolOperationReadNotebook, protocol.ToolOperationEditNotebook, protocol.ToolOperationReadArtifact, protocol.ToolOperationListArtifacts, protocol.ToolOperationListArtifactRevisions, protocol.ToolOperationReadGitLog, protocol.ToolOperationReadGitShow, protocol.ToolOperationReadGitBlame, protocol.ToolOperationListGitChanges, protocol.ToolOperationReadGitChange, protocol.ToolOperationListWorktrees, protocol.ToolOperationProgress, protocol.ToolOperationListRoles:
		return true
	}
	return false
}
