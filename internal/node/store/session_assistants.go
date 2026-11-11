package store

import (
	"context"
	"database/sql"
	"errors"
	"slices"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// assistantDeclaration resolves assistant policy only from its admitted
// immutable package version.
func assistantDeclaration(binding protocol.PluginConversationBinding, owner protocol.PluginInfo) (protocol.PluginConversationDeclaration, error) {
	if owner.Extension != nil && owner.Extension.Desktop != nil {
		for _, declaration := range owner.Extension.Desktop.Conversations {
			if declaration.ID == binding.ID {
				return declaration, nil
			}
		}
	}
	return protocol.PluginConversationDeclaration{}, fault(protocol.ErrorCodeNotConfigured, "plugin assistant declaration is unavailable")
}

// ReadAssistantDeclaration returns the declaration frozen by a session's
// assistant binding.
func ReadAssistantDeclaration(ctx context.Context, db *sql.Conn, binding protocol.PluginConversationBinding) (protocol.PluginConversationDeclaration, error) {
	owner, err := ReadPluginVersion(ctx, db, binding.Package)
	if err != nil {
		return protocol.PluginConversationDeclaration{}, err
	}
	return assistantDeclaration(binding, owner)
}

func captureAssistant(ctx context.Context, db *sql.Conn, invocation *AgentInvocation) error {
	binding := invocation.Turn.Config.Assistant
	if binding == nil {
		return nil
	}
	declaration, err := ReadAssistantDeclaration(ctx, db, *binding)
	invocation.Assistant = &declaration
	return err
}

// validateAssistant admits a new assistant conversation only for the current
// package version and the resource its declaration names.
func validateAssistant(ctx context.Context, db *sql.Conn, project *protocol.ProjectID, config protocol.SessionConfig) error {
	binding := config.Assistant
	if binding == nil {
		return nil
	}
	current, err := ReadPlugin(ctx, db, binding.Package.Name)
	if err != nil {
		return err
	}
	if current == nil || !current.Summary.Enabled || current.Summary.Compatibility.Kind != "compatible" {
		return fault(protocol.ErrorCodeNotConfigured, "assistant plugin is unavailable")
	}
	if plugins.PackageReference(*current) != binding.Package {
		return fault(protocol.ErrorCodeRevisionConflict, "plugin assistant version changed")
	}
	declaration, err := ReadAssistantDeclaration(ctx, db, *binding)
	if err != nil {
		return err
	}
	matches := project == nil
	switch declaration.Resource {
	case protocol.PluginConversationResourcePlugin:
		matches = matches && config.Resource == nil
	case protocol.PluginConversationResourceWorkspace:
		matches = project != nil && config.Resource == nil
	case protocol.PluginConversationResourceDatabase:
		matches = matches && config.Resource != nil && config.Resource.Kind == "database"
	case protocol.PluginConversationResourceSsh:
		matches = matches && config.Resource != nil && config.Resource.Kind == "ssh"
	}
	if !matches {
		return fault(protocol.ErrorCodePermissionDenied, "conversation resource does not match the plugin assistant")
	}
	return nil
}

// assistantWorkspace returns the one scratch worktree every conversation of a
// plugin-resource assistant shares.
func assistantWorkspace(ctx context.Context, db *sql.Conn, profile string, binding protocol.PluginConversationBinding) (protocol.WorktreeID, error) {
	declaration, err := ReadAssistantDeclaration(ctx, db, binding)
	if err != nil {
		return protocol.WorktreeID{}, err
	}
	if declaration.Resource != protocol.PluginConversationResourcePlugin {
		return protocol.WorktreeID{}, invalid("plugin assistant requires its declared resource")
	}
	root, err := core.AssistantWorkspace(profile, binding)
	if err != nil {
		return protocol.WorktreeID{}, err
	}
	var existing string
	err = db.QueryRowContext(ctx, "SELECT id FROM worktrees WHERE project IS NULL AND path=?1", root).Scan(&existing)
	if err == nil {
		return protocol.ParseWorktreeID(existing)
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return protocol.WorktreeID{}, err
	}
	id := protocol.NewWorktreeID()
	_, err = db.ExecContext(ctx, "INSERT INTO worktrees(id,project,path,main) VALUES(?1,NULL,?2,0)", id.String(), root)
	return id, err
}

// assistantPackages resolves the owner and every package whose tool the
// assistant imports. Admitted work keeps the owner's captured version.
func assistantPackages(ctx context.Context, db *sql.Conn, binding protocol.PluginConversationBinding) ([]protocol.PluginInfo, error) {
	current, err := ReadPlugin(ctx, db, binding.Package.Name)
	if err != nil {
		return nil, err
	}
	if current == nil || !current.Summary.Enabled || current.Summary.Compatibility.Kind != "compatible" {
		return nil, fault(protocol.ErrorCodeNotConfigured, "assistant plugin is disabled or removed")
	}
	owner, err := ReadPluginVersion(ctx, db, binding.Package)
	if err != nil {
		return nil, err
	}
	declaration, err := assistantDeclaration(binding, owner)
	if err != nil {
		return nil, err
	}
	references := []protocol.PluginReference{binding.Package}
	packages := []protocol.PluginInfo{owner}
	for _, tool := range declaration.Tools {
		if tool.Kind != "package" {
			continue
		}
		imported, valid := dispatchValue[protocol.PluginConversationToolPackagePayload](tool.Data)
		if !valid {
			return nil, invalid("invalid assistant package tool")
		}
		info := owner
		if imported.Package != binding.Package.Name {
			selected, err := ReadPlugin(ctx, db, imported.Package)
			if err != nil {
				return nil, err
			}
			if selected == nil || !selected.Summary.Enabled || selected.Summary.Compatibility.Kind != "compatible" {
				return nil, fault(protocol.ErrorCodeNotConfigured, "assistant package tool is unavailable")
			}
			info = *selected
		}
		if info.Extension == nil || !slices.ContainsFunc(info.Extension.Tools, func(candidate protocol.ToolDeclaration) bool {
			return candidate.Name == imported.Name && candidate.Server == nil && (candidate.Operation != nil || candidate.Handler != nil)
		}) {
			return nil, fault(protocol.ErrorCodeNotConfigured, "assistant package tool is unavailable")
		}
		reference := plugins.PackageReference(info)
		if !slices.Contains(references, reference) {
			captured, err := ReadPluginVersion(ctx, db, reference)
			if err != nil {
				return nil, err
			}
			references, packages = append(references, reference), append(packages, captured)
		}
	}
	return packages, nil
}

// validateAssistantDelegation rejects an agent reference before its turn is
// admitted unless the assistant selects the delegation tool or a package tool
// bound to the delegation operation. captured names the packages of an already
// admitted turn; a new turn resolves the assistant's current packages.
func validateAssistantDelegation(ctx context.Context, db *sql.Conn, binding *protocol.PluginConversationBinding, message protocol.ConversationInput, captured []protocol.PluginReference) error {
	if binding == nil || !slices.ContainsFunc(message.References, func(reference protocol.ConversationReference) bool { return reference.Target.Kind == "agent" }) {
		return nil
	}
	denied := fault(protocol.ErrorCodePermissionDenied, "plugin assistant does not allow delegation")
	var packages []protocol.PluginInfo
	var owner *protocol.PluginInfo
	if captured == nil {
		current, err := assistantPackages(ctx, db, *binding)
		if err != nil {
			return err
		}
		packages, owner = current, &current[0]
	} else {
		for _, reference := range captured {
			info, err := ReadPluginVersion(ctx, db, reference)
			if err != nil {
				return err
			}
			packages = append(packages, info)
			if reference == binding.Package {
				owner = &packages[len(packages)-1]
			}
		}
	}
	if owner == nil {
		return denied
	}
	declaration, err := assistantDeclaration(*binding, *owner)
	if err != nil {
		return denied
	}
	for _, tool := range declaration.Tools {
		provider, name := binding.Package.Name, ""
		switch tool.Kind {
		case "builtin":
			if builtin, _ := dispatchValue[protocol.PluginConversationToolBuiltinPayload](tool.Data); builtin.Name == "spawn_agent" {
				return nil
			}
			continue
		case "plugin":
			selected, _ := dispatchValue[protocol.PluginConversationToolPluginPayload](tool.Data)
			if selected.Server != nil {
				continue
			}
			name = selected.Name
		case "package":
			selected, _ := dispatchValue[protocol.PluginConversationToolPackagePayload](tool.Data)
			provider, name = selected.Package, selected.Name
		default:
			continue
		}
		for _, info := range packages {
			if info.Summary.Name != provider || info.Extension == nil {
				continue
			}
			for _, candidate := range info.Extension.Tools {
				operation := candidate.Operation
				if operation == nil && candidate.Handler != nil {
					operation = candidate.Handler.Operation
				}
				if candidate.Name == name && operation != nil && *operation == protocol.ToolOperationDelegateAgent {
					return nil
				}
			}
		}
	}
	return denied
}
