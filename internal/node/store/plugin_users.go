package store

import (
	"context"
	"database/sql"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// initializePluginUsers moves legacy mutable state to the local user after the
// additive schema upgrade. Removing the legacy rows makes this idempotent and
// prevents old, unscoped readers from exposing another user's live state.
func initializePluginUsers(ctx context.Context, db *sql.Conn) error {
	user, err := LocalUser(ctx, db)
	if err != nil {
		return err
	}
	for _, statement := range []string{
		"INSERT INTO plugin_user_installations(user,name,revision,settings_revision,installed,body) SELECT ?1,name,revision,settings_revision,installed,body FROM plugins WHERE true ON CONFLICT(user,name) DO NOTHING",
		"INSERT INTO plugin_user_settings(user,plugin,revision,body) SELECT ?1,plugin,revision,body FROM plugin_settings WHERE true ON CONFLICT(user,plugin,revision) DO NOTHING",
		"INSERT INTO plugin_space_values(space,name,key,revision,value,index_data) SELECT 'user:'||?1,name,key,revision,value,index_data FROM plugin_values WHERE true ON CONFLICT(space,name,key) DO NOTHING",
		"INSERT INTO plugin_search_spaces(space,name) SELECT 'user:'||?1,name FROM plugin_values WHERE index_data IS NOT NULL ON CONFLICT(space,name) DO NOTHING",
		"INSERT INTO plugin_search_spaces(space,name) SELECT 'user:'||?1,name FROM plugins WHERE true ON CONFLICT(space,name) DO NOTHING",
		"UPDATE plugin_connections SET user=?1 WHERE user IS NULL",
		"INSERT INTO plugin_connection_captures(connection,user) SELECT id,user FROM plugin_connections WHERE user IS NOT NULL ON CONFLICT(connection,user) DO NOTHING",
		"UPDATE knowledge_collections SET body=json_set(body,'$.capture.user',?1) WHERE json_extract(body,'$.capture.user') IS NULL",
		"UPDATE knowledge_collections SET body=json_set(body,'$.capture.space','user:'||?1) WHERE json_extract(body,'$.collection.definition.source.kind')='storage' AND json_extract(body,'$.collection.definition.source.scope')='node' AND json_extract(body,'$.capture.space') IS NULL",
	} {
		if _, err := db.ExecContext(ctx, statement, user.ID.String()); err != nil {
			return err
		}
	}
	for _, table := range []string{"plugins", "plugin_settings", "plugin_values"} {
		if _, err := db.ExecContext(ctx, "DELETE FROM "+table); err != nil {
			return err
		}
	}
	return initializeDispatchUsers(ctx, db)
}

func knowledgeUserAllowed(ctx context.Context, db *sql.Conn, stored KnowledgeStored) (bool, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return false, err
	}
	owned := stored.Capture.User == nil && user.Local
	if stored.Capture.User != nil {
		owner, err := ResolveUser(ctx, db, *stored.Capture.User)
		if err != nil {
			return false, err
		}
		owned = owner.ID == user.ID
	}
	var allowed bool
	if project, ok := strings.CutPrefix(stored.Capture.Space, "project:"); ok {
		err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM projects p WHERE p.id=?1 AND (p.owner=?2 OR p.visibility='shared') AND NOT EXISTS(SELECT 1 FROM removed_projects r WHERE r.project=p.id))", project, user.ID.String()).Scan(&allowed)
		return allowed, err
	}
	if stored.Capture.Worktree != nil {
		var project sql.NullString
		err = db.QueryRowContext(ctx, "SELECT project FROM worktrees w WHERE w.id=?1 AND NOT EXISTS(SELECT 1 FROM removed_worktrees r WHERE r.worktree=w.id)", stored.Capture.Worktree.String()).Scan(&project)
		if err == sql.ErrNoRows {
			return false, nil
		}
		if err != nil {
			return false, err
		}
		if project.Valid {
			err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM projects p WHERE p.id=?1 AND (p.owner=?2 OR p.visibility='shared') AND NOT EXISTS(SELECT 1 FROM removed_projects r WHERE r.project=p.id))", project.String, user.ID.String()).Scan(&allowed)
			return allowed, err
		}
	}
	return owned, nil
}

// KnowledgeExecutionContext restores the owner of a trusted indexing capture.
// Public queries select allowed collections before the indexing service uses it.
func KnowledgeExecutionContext(ctx context.Context, db *sql.Conn, stored KnowledgeStored) (context.Context, error) {
	if stored.Capture.User != nil {
		user, err := ResolveUser(ctx, db, *stored.Capture.User)
		if err != nil {
			return nil, err
		}
		ctx = WithCapturedUser(WithUser(ctx, user), *stored.Capture.User)
	}
	return knowledgeStorageContext(ctx, db, stored)
}

func knowledgeStorageContext(ctx context.Context, db *sql.Conn, stored KnowledgeStored) (context.Context, error) {
	allowed, err := knowledgeUserAllowed(ctx, db, stored)
	if err != nil {
		return nil, err
	}
	if !allowed {
		return nil, fault(protocol.ErrorCodeNotFound, "knowledge collection is unavailable")
	}
	if stored.Capture.Space != "" {
		ctx = context.WithValue(ctx, pluginSpaceKey{}, stored.Capture.Space)
	}
	return ctx, nil
}

func pluginUserID(ctx context.Context, db *sql.Conn) (string, error) {
	user, err := CurrentUser(ctx, db)
	return user.ID.String(), err
}

func pluginCapturedUserID(ctx context.Context, db *sql.Conn) (string, error) {
	user, err := CapturedUser(ctx, db)
	return user.String(), err
}

type pluginSpaceKey struct{}

// pluginValueSpace is trusted execution context, never a namespace supplied by
// a plugin. Project storage commands resolve their project before setting it.
func pluginValueSpace(ctx context.Context, db *sql.Conn) (string, error) {
	if space, ok := ctx.Value(pluginSpaceKey{}).(string); ok {
		return space, nil
	}
	user, err := CapturedUser(ctx, db)
	if err != nil {
		return "", err
	}
	return "user:" + user.String(), nil
}

func pluginValueScope(ctx context.Context) protocol.PluginStorageScope {
	if space, ok := ctx.Value(pluginSpaceKey{}).(string); ok && strings.HasPrefix(space, "project:") {
		return protocol.PluginStorageScopeProject
	}
	return protocol.PluginStorageScopeNode
}

func pluginSearchName(ctx context.Context, db *sql.Conn, name string) (string, error) {
	space, err := pluginValueSpace(ctx, db)
	if err != nil {
		return "", err
	}
	local, err := LegacyStorageUser(ctx, db)
	if err != nil {
		return "", err
	}
	if space == "user:"+local.String() {
		return name, nil
	}
	return space + "\x00" + name, nil
}

func pluginStorageContext(ctx context.Context, db *sql.Conn, scope protocol.PluginContext, storage protocol.PluginStorageScope) (context.Context, error) {
	if storage == "" || storage == protocol.PluginStorageScopeNode {
		user, err := CapturedUser(ctx, db)
		if err != nil {
			return nil, err
		}
		return context.WithValue(ctx, pluginSpaceKey{}, "user:"+user.String()), nil
	}
	if storage != protocol.PluginStorageScopeProject || scope.Worktree == nil {
		return nil, fault(protocol.ErrorCodePermissionDenied, "plugin project storage requires a worktree")
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, err
	}
	var project string
	err = db.QueryRowContext(ctx, `SELECT p.id FROM worktrees w JOIN projects p ON p.id=w.project
WHERE w.id=?1 AND (p.owner=?2 OR p.visibility='shared')
AND NOT EXISTS(SELECT 1 FROM removed_projects r WHERE r.project=p.id)
AND NOT EXISTS(SELECT 1 FROM removed_worktrees r WHERE r.worktree=w.id)`, scope.Worktree.String(), user.ID.String()).Scan(&project)
	if err == sql.ErrNoRows {
		return nil, fault(protocol.ErrorCodeNotFound, "plugin project storage is unavailable")
	}
	if err != nil {
		return nil, err
	}
	return context.WithValue(ctx, pluginSpaceKey{}, "project:"+project), nil
}

// PluginStorageRequestContext resolves the requested namespace through the
// captured resource. The personal namespace remains the default for old SDKs.
func PluginStorageRequestContext(ctx context.Context, db *sql.Conn, request protocol.Request) (context.Context, error) {
	var storage protocol.PluginStorageScope
	switch input := request.Command.Data.(type) {
	case *protocol.CommandReadPluginValuePayload:
		storage = input.Scope
	case *protocol.CommandWritePluginValuePayload:
		storage = input.Scope
	case *protocol.CommandWriteIndexedPluginValuePayload:
		storage = input.Scope
	case *protocol.CommandRemovePluginValuePayload:
		storage = input.Scope
	case *protocol.CommandListPluginKeysPayload:
		storage = input.Scope
	case *protocol.PluginStorageSearch:
		storage = input.Scope
	case *protocol.CommandContinueTurnPayload:
		if input.Scope == protocol.PluginStorageScopeConversation {
			return ctx, nil
		}
		storage = input.Scope
	default:
		return ctx, nil
	}
	if request.Plugin == nil {
		return nil, fault(protocol.ErrorCodePermissionDenied, "plugin provenance is required")
	}
	return pluginStorageContext(ctx, db, *request.Plugin, storage)
}

func connectorUserIDs(ctx context.Context, db *sql.Conn) (string, string, error) {
	captured, err := CapturedUser(ctx, db)
	if err != nil {
		return "", "", err
	}
	current, err := CurrentUser(ctx, db)
	if err != nil {
		return "", "", err
	}
	owner, err := ResolveUser(ctx, db, captured)
	if err != nil {
		return "", "", err
	}
	if owner.ID != current.ID {
		return "", "", fault(protocol.ErrorCodePermissionDenied, "connection storage principal differs from its actor")
	}
	return captured.String(), current.ID.String(), nil
}
