package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func SetPluginPinned(ctx context.Context, db *sql.Conn, name string, expectedRevision uint64, pinned bool) (protocol.PluginInfo, error) {
	if err := CheckConfigurationEditable(ctx, db, "plugins", name); err != nil {
		return protocol.PluginInfo{}, err
	}
	info, err := ReadPlugin(ctx, db, name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if info == nil {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotFound, "plugin is not installed")
	}
	if err := checkRevision(info.Summary.Revision, expectedRevision); err != nil {
		return protocol.PluginInfo{}, err
	}
	if info.Summary.Pinned == pinned {
		return *info, nil
	}
	info.Summary.Revision, err = nextRevision(info.Summary.Revision)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	info.Summary.Pinned = pinned
	return *info, savePluginInventory(ctx, db, *info, true)
}

func validateCatalogReplacement(ctx context.Context, db *sql.Conn, current *protocol.PluginInfo, next *protocol.PluginInfo, mode protocol.PluginInstallMode, declared protocol.PluginMcpConfiguration) error {
	if mode == protocol.PluginInstallModeInstall && current != nil {
		return fault(protocol.ErrorCodeRevisionConflict, "plugin is already installed")
	}
	if mode == protocol.PluginInstallModeUpdate {
		if current == nil {
			return fault(protocol.ErrorCodeNotFound, "plugin is not installed")
		}
		if current.Summary.Pinned {
			return invalid("plugin version is pinned")
		}
		if !plugins.IsNewer(current.Summary.Version, next.Summary.Version) {
			return invalid("plugin update requires a strictly newer version")
		}
	}
	if current == nil {
		return nil
	}
	if current.Summary.SettingsRevision != 0 {
		stored, err := readStoredPluginSettings(ctx, db, *current)
		if err != nil {
			return err
		}
		configuration, err := EffectivePluginMCP(ResolvedPluginSettings{MCP: stored.Mcp}, declared)
		if err != nil {
			return err
		}
		if len(next.Mcp) != 0 || len(configuration.Servers) != 0 {
			if err := validateMCPConfiguration(*next, configuration); err != nil {
				return err
			}
		}
		if err := validateVersionSettings(stored, next.Settings, MCPSettingsTargets(next.Settings, configuration)); err != nil {
			return err
		}
	}
	if mode == protocol.PluginInstallModeSwitch && plugins.IsNewer(next.Summary.Version, current.Summary.Version) {
		if err := validateDowngradeStorage(ctx, db, *next); err != nil {
			return err
		}
		next.Summary.Pinned = true
	}
	return nil
}

func validateVersionSettings(stored storedPluginSettings, schema *protocol.PluginSettingsSchema, targets map[string]string) error {
	problem := func() error { return invalid("selected plugin version cannot accept the current configuration") }
	if schema == nil {
		if len(stored.Values) != 0 {
			return problem()
		}
		for _, secret := range stored.Slots {
			if secret != nil {
				return problem()
			}
		}
		return nil
	}
	values := plugins.SettingDefaults(schema)
	for name, value := range stored.Values {
		// Removed fields must not silently disappear through the settings projection.
		field, exists := schema.Properties[name]
		if !exists || field.Secret != nil {
			return problem()
		}
		values[name] = value
	}
	if plugins.ValidatePublicSettings(*schema, values) != nil {
		return problem()
	}
	for name, secret := range stored.Slots {
		if secret == nil {
			continue
		}
		previous, target := settingsBinding(stored.Schema, name), settingsBinding(schema, name)
		if previous == nil || target == nil || target.Unsupported || !reflect.DeepEqual(previous, target) {
			return problem()
		}
		if target.Server != nil && (targets[name] == "" || stored.McpTargets[name] != targets[name]) {
			return problem()
		}
		field := schema.Properties[name]
		field.Secret, field.Default = nil, nil
		raw, _ := json.Marshal(secret.Expose())
		check := protocol.PluginSettingsSchema{Schema: plugins.SettingsSchema, Kind: protocol.PluginSettingsObjectObject,
			Properties: map[string]protocol.PluginSettingsField{name: field}, Required: []string{name}}
		if plugins.ValidatePublicSettings(check, map[string]json.RawMessage{name: raw}) != nil {
			return problem()
		}
	}
	for _, name := range schema.Required {
		if schema.Properties[name].Secret != nil && stored.Slots[name] == nil {
			return problem()
		}
	}
	return nil
}

// Downgrades validate only this user's personal data and resources they can access.
// Validation does not rewrite values, restore older settings or retain extra packages.
func validateDowngradeStorage(ctx context.Context, db *sql.Conn, next protocol.PluginInfo) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	space, err := personalPluginSpace(ctx, db, user.ID)
	if err != nil {
		return err
	}
	rows, err := db.QueryContext(ctx, `SELECT 'node',key,value FROM plugin_space_values WHERE name=?1 AND space=?3 AND value IS NOT NULL
UNION ALL SELECT 'project',v.key,v.value FROM plugin_space_values v JOIN projects p ON v.space='project:'||p.id
WHERE v.name=?1 AND v.value IS NOT NULL AND (p.visibility='shared' OR coalesce(p.owner,(SELECT id FROM users WHERE local=1))=?2)
AND NOT EXISTS(SELECT 1 FROM removed_projects r WHERE r.project=p.id)
UNION ALL SELECT 'conversation',v.key,r.value FROM plugin_conversation_values v
JOIN plugin_conversation_records r ON r.sequence=v.record JOIN sessions s ON s.id=v.session
LEFT JOIN projects p ON p.id=s.project WHERE v.name=?1 AND r.value IS NOT NULL AND (`+sessionAccessPredicate+`)`,
		next.Summary.Name, user.ID.String(), space)
	if err != nil {
		return err
	}
	defer rows.Close()
	for rows.Next() {
		var scope protocol.PluginStorageScope
		var key string
		var value []byte
		if err := rows.Scan(&scope, &key, &value); err != nil {
			return err
		}
		collection := plugins.StorageCollection(next, scope, key)
		if collection == nil && scope == protocol.PluginStorageScopeProject {
			return invalid("selected plugin version does not declare the existing project data")
		}
		if _, err := plugins.NormalizeStorageValue(collection, nil, value); err != nil {
			return invalid("selected plugin version cannot accept the current stored data")
		}
	}
	return rows.Err()
}
