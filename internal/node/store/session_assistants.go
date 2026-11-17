package store

import (
	"context"
	"database/sql"
	"errors"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// assistantDeclaration resolves an assistant ID in a package version.
func assistantDeclaration(binding protocol.PluginConversationBinding, owner protocol.PluginInfo) (protocol.PluginConversationDeclaration, error) {
	if owner.Extension != nil && owner.Extension.Desktop != nil {
		for _, declaration := range owner.Extension.Desktop.Conversations {
			if declaration.ID == binding.ID {
				return declaration, nil
			}
		}
	}
	return protocol.PluginConversationDeclaration{}, protocol.NewConfigurationFault(protocol.ConfigurationPluginDeclarationMissing, "plugin assistant declaration is unavailable")
}

// ReadAssistantDeclaration returns the declaration in an exact package capture.
func ReadAssistantDeclaration(ctx context.Context, db *sql.Conn, binding protocol.PluginConversationBinding) (protocol.PluginConversationDeclaration, error) {
	owner, err := ReadPluginVersion(ctx, db, binding.Package)
	if err != nil {
		return protocol.PluginConversationDeclaration{}, err
	}
	return assistantDeclaration(binding, owner)
}

func captureAssistant(ctx context.Context, db *sql.Conn, invocation *AgentInvocation) error {
	if err := bindTurnAssistant(&invocation.Turn); err != nil {
		return err
	}
	binding := invocation.Turn.Config.Assistant
	if binding == nil {
		return nil
	}
	declaration, err := ReadAssistantDeclaration(ctx, db, *binding)
	invocation.Assistant = &declaration
	return err
}

// bindTurnAssistant resolves the session's assistant identity through this
// turn's frozen inventory. Copy the binding: a session revision can also be
// held by an older running turn and is never updated by this projection.
func bindTurnAssistant(turn *protocol.QueuedTurn) error {
	if turn.Config.Assistant == nil {
		return nil
	}
	for _, reference := range turn.Plugins {
		if reference.Name == turn.Config.Assistant.Package.Name {
			binding := *turn.Config.Assistant
			binding.Package = reference
			turn.Config.Assistant = &binding
			return nil
		}
	}
	return protocol.NewConfigurationFault(protocol.ConfigurationAssistantCaptureMissing, "assistant plugin is outside the captured turn")
}

// validateAssistant admits an assistant capture only for the current package
// version and the resource its declaration names.
func validateAssistant(ctx context.Context, db *sql.Conn, project *protocol.ProjectID, config protocol.SessionConfig) error {
	binding := config.Assistant
	if binding == nil {
		return nil
	}
	current, err := ReadPlugin(ctx, db, binding.Package.Name)
	if err != nil {
		return err
	}
	if current != nil && !current.Summary.Enabled {
		return protocol.NewConfigurationFault(protocol.ConfigurationPluginDisabled, "assistant plugin is unavailable")
	}
	if current == nil || current.Summary.Compatibility.Kind != "compatible" {
		return protocol.NewConfigurationFault(protocol.ConfigurationPluginUnavailable, "assistant plugin is unavailable")
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
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return protocol.WorktreeID{}, err
	}
	workspaceProfile := profile
	if !user.Local {
		workspaceProfile = filepath.Join(profile, "users", user.ID.String())
	}
	root, err := core.AssistantWorkspace(workspaceProfile, binding)
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
	_, err = db.ExecContext(ctx, "INSERT INTO worktrees(id,project,path,main,owner) VALUES(?1,NULL,?2,0,?3)", id.String(), root, user.ID.String())
	return id, err
}

// assistantPackage resolves the current package for a new admission. Existing
// turns read their declaration only through their frozen package references.
func assistantPackage(ctx context.Context, db *sql.Conn, binding protocol.PluginConversationBinding) (protocol.PluginInfo, error) {
	current, err := ReadPlugin(ctx, db, binding.Package.Name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if current != nil && !current.Summary.Enabled {
		return protocol.PluginInfo{}, protocol.NewConfigurationFault(protocol.ConfigurationPluginDisabled, "assistant plugin is disabled or removed")
	}
	if current == nil || current.Summary.Compatibility.Kind != "compatible" {
		return protocol.PluginInfo{}, protocol.NewConfigurationFault(protocol.ConfigurationPluginUnavailable, "assistant plugin is disabled or removed")
	}
	if _, err := assistantDeclaration(binding, *current); err != nil {
		return protocol.PluginInfo{}, err
	}
	return *current, nil
}
