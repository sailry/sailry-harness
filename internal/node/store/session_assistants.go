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
