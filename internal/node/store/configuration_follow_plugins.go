package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"maps"
	"reflect"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginFollowSettings struct {
	Package protocol.PluginReference        `json:"package"`
	Values  map[string]json.RawMessage      `json:"values"`
	MCP     protocol.PluginMcpConfiguration `json:"mcp"`
	Allowed map[string]bool                 `json:"allowed"`
}
type pluginFollowSecrets struct {
	Slots map[string]protocol.Secret `json:"slots"`
}
type pluginFollowConnection struct {
	Package       protocol.PluginReference          `json:"package"`
	ID            protocol.PluginConnectorID        `json:"id"`
	Connector     string                            `json:"connector"`
	Method        protocol.PluginConnectorKind      `json:"method"`
	Enabled       bool                              `json:"enabled"`
	Fields        map[string]string                 `json:"fields"`
	Revision      uint64                            `json:"revision"`
	AuthRevision  uint64                            `json:"auth_revision"`
	GrantRevision uint64                            `json:"grant_revision"`
	Identity      *protocol.PluginConnectorIdentity `json:"identity,omitempty"`
	Bound         *ConnectorBound                   `json:"bound,omitempty"`
	State         protocol.PluginConnectorState     `json:"state"`
	CheckedAtMs   *uint64                           `json:"checked_at_ms,omitempty"`
	Login         *protocol.RequestID               `json:"login,omitempty"`
}

func (value pluginFollowConnection) runtimeRecord() ConnectorRecord {
	return ConnectorRecord{
		ID: value.ID, Plugin: value.Package.Name, Connector: value.Connector, Revision: value.Revision,
		AuthRevision: value.AuthRevision, GrantRevision: value.GrantRevision, Method: value.Method,
		Enabled: value.Enabled, Fields: value.Fields, Identity: value.Identity, Bound: value.Bound,
		State: value.State, CheckedAtMs: value.CheckedAtMs, Login: value.Login,
	}
}

// CacheConfigurationPackage records metadata only after Node has materialized
// and verified the content-addressed package outside the category transaction.
func CacheConfigurationPackage(ctx context.Context, db *sql.Conn, info protocol.PluginInfo, declared protocol.PluginMcpConfiguration) error {
	if !plugins.ValidateName(info.Summary.Name) || len(info.Summary.Digest) != 64 {
		return invalid("invalid followed package identity")
	}
	if len(info.Mcp) > 0 {
		if err := validateMCPConfiguration(info, declared); err != nil {
			return err
		}
	}
	info.Summary.Source = nil
	body, err := encode(info)
	if err != nil {
		return err
	}
	mcp, err := encode(declared)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_packages(digest,name,body,mcp) VALUES(?1,?2,?3,?4) ON CONFLICT(digest) DO UPDATE SET body=excluded.body,mcp=excluded.mcp WHERE name=excluded.name", info.Summary.Digest, info.Summary.Name, body, mcp)
	return err
}
func followedPackage(ctx context.Context, db *sql.Conn, reference protocol.PluginReference) (protocol.PluginInfo, protocol.PluginMcpConfiguration, error) {
	var info protocol.PluginInfo
	var declared protocol.PluginMcpConfiguration
	var body, mcp []byte
	err := db.QueryRowContext(ctx, "SELECT body,mcp FROM configuration_follow_packages WHERE digest=?1 AND name=?2", reference.Digest, reference.Name).Scan(&body, &mcp)
	if errors.Is(err, sql.ErrNoRows) {
		err = fault(protocol.ErrorCodeNotConfigured, "plugin package content is pending")
	}
	if err != nil {
		return info, declared, err
	}
	if err = json.Unmarshal(body, &info); err != nil {
		return info, declared, err
	}
	err = json.Unmarshal(mcp, &declared)
	return info, declared, err
}
func exportPluginFollowRecords(ctx context.Context, db *sql.Conn, category string, includeSecrets bool, declared map[string]protocol.PluginMcpConfiguration) ([]protocol.ConfigurationFollowRecord, error) {
	records := []protocol.ConfigurationFollowRecord{}
	add := func(id string, revision uint64, value any, secret any) error {
		origin, err := configurationOrigin(ctx, db, category, id)
		if err != nil {
			return err
		}
		if origin != nil {
			return nil
		}
		body, err := encode(value)
		if err != nil {
			return err
		}
		record := protocol.ConfigurationFollowRecord{ID: id, Revision: max(1, revision), Body: body}
		if secret != nil {
			raw, err := encode(secret)
			if err != nil {
				return err
			}
			value := protocol.NewSecret(string(raw))
			record.Secret = &value
		}
		record.Digest, err = configurationDigest(record)
		if err != nil {
			return err
		}
		records = append(records, record)
		return nil
	}
	infos, err := ReadPluginInfos(ctx, db)
	if err != nil {
		return nil, err
	}
	for _, info := range infos {
		if origin, err := configurationOrigin(ctx, db, "plugins", info.Summary.Name); err != nil {
			return nil, err
		} else if origin != nil {
			continue
		}
		switch category {
		case "plugins":
			info.Summary.Source = nil
			if err := add(info.Summary.Name, info.Summary.Revision, info, nil); err != nil {
				return nil, err
			}
		case "plugin_settings":
			settings, err := ResolvePluginSettings(ctx, db, plugins.PackageReference(info))
			if err != nil {
				return nil, err
			}
			mcp := declared[info.Summary.Name]
			if len(info.Mcp) > 0 {
				mcp, err = EffectivePluginMCP(settings, mcp)
				if err != nil {
					return nil, err
				}
			}
			value := pluginFollowSettings{Package: plugins.PackageReference(info), Values: settings.Values, MCP: mcp, Allowed: settings.MCPAllowed}
			var secret any
			if includeSecrets && len(settings.Secrets) > 0 {
				secret = pluginFollowSecrets{Slots: settings.Secrets}
			}
			revision, err := nextRevision(info.Summary.SettingsRevision)
			if err != nil {
				return nil, err
			}
			if err := add(info.Summary.Name, revision, value, secret); err != nil {
				return nil, err
			}
		case "connections":
			user, err := CurrentUser(ctx, db)
			if err != nil {
				return nil, err
			}
			values, err := readBodies[ConnectorRecord](ctx, db, "SELECT body FROM plugin_connections WHERE user=?1 AND plugin=?2 ORDER BY id", user.ID.String(), info.Summary.Name)
			if err != nil {
				return nil, err
			}
			for _, record := range values {
				value := pluginFollowConnection{
					Package: plugins.PackageReference(info), ID: record.ID, Connector: record.Connector, Method: record.Method,
					Enabled: record.Enabled, Fields: record.Fields, Revision: record.Revision, AuthRevision: record.AuthRevision,
					GrantRevision: record.GrantRevision, Identity: record.Identity, Bound: record.Bound, State: record.State,
					CheckedAtMs: record.CheckedAtMs, Login: record.Login,
				}
				value.Fields = maps.Clone(value.Fields)
				for name := range value.Fields {
					if !providers.RuntimeField(name) {
						delete(value.Fields, name)
					}
				}
				if err := add(record.ID.String(), record.Revision, value, nil); err != nil {
					return nil, err
				}
			}
		}
	}
	slices.SortFunc(records, func(a, b protocol.ConfigurationFollowRecord) int { return strings.Compare(a.ID, b.ID) })
	return records, nil
}

func pluginFollowCollision(ctx context.Context, db *sql.Conn, category string, record protocol.ConfigurationFollowRecord) error {
	id := record.ID
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	var exists bool
	if category == "connections" {
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM plugin_connections WHERE id=?1 AND user<>?2)", id, user.ID.String()).Scan(&exists); err != nil {
			return err
		}
		if exists {
			return fault(protocol.ErrorCodeConflict, "source identity belongs to another user")
		}
	}
	if origin, err := configurationOrigin(ctx, db, category, id); err != nil {
		return err
	} else if origin != nil {
		return checkConfigurationCopySource(ctx, db, origin)
	}
	switch category {
	case "plugins":
		var incoming protocol.PluginInfo
		if err := decodeConfigurationRecord(record, &incoming); err != nil {
			return err
		}
		current, err := ReadPlugin(ctx, db, id)
		if err != nil {
			return err
		}
		// The controller owns this separate receiving namespace, including its
		// initial bundled packages. Adopting its exact artifact is an explicit
		// mirror operation, independent of automatic SemVer update rules.
		if !user.Local {
			return nil
		}
		// Local-only legacy follow keeps its original collision rules.
		if current != nil && current.Origin != nil && current.Origin.Kind == "bundled" && incoming.Origin != nil && incoming.Origin.Kind == "bundled" && current.Summary.Digest == incoming.Summary.Digest && current.Summary.Revision == 1 && current.Summary.SettingsRevision == 0 {
			return nil
		}
		err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM plugin_user_installations WHERE user=?1 AND name=?2)", user.ID.String(), id).Scan(&exists)
	case "plugin_settings":
		// Settings follow the package accepted from this source, never an unrelated
		// local installation having the same name or an explicit removal choice.
		origin, err := configurationOrigin(ctx, db, "plugins", id)
		if err != nil {
			return err
		}
		if origin == nil {
			return fault(protocol.ErrorCodeNotConfigured, "source plugin installation is pending")
		}
		return nil
	case "connections":
		if !user.Local {
			return nil
		}
		err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM plugin_connections WHERE id=?1)", id).Scan(&exists)
	}
	if err != nil {
		return err
	}
	if exists {
		return fault(protocol.ErrorCodeConflict, "source plugin record conflicts with local configuration")
	}
	return nil
}
func applyPluginFollowRecord(ctx context.Context, db *sql.Conn, node protocol.NodeID, category string, record protocol.ConfigurationFollowRecord) ([]protocol.Event, bool, error) {
	if err := pluginFollowCollision(ctx, db, category, record); err != nil {
		return nil, false, err
	}
	switch category {
	case "plugins":
		var source protocol.PluginInfo
		if err := decodeConfigurationRecord(record, &source); err != nil {
			return nil, false, err
		}
		if source.Summary.Name != record.ID {
			return nil, false, invalid("plugin name does not match source manifest")
		}
		info, _, err := followedPackage(ctx, db, plugins.PackageReference(source))
		if err != nil {
			return nil, false, err
		}
		info.McpSource = source.McpSource
		if source.Origin != nil && (source.Origin.Kind == "bundled" || source.Origin.Kind == "online" || source.Origin.Kind == "registry") {
			info.Origin = source.Origin
		}
		old, err := ReadPlugin(ctx, db, record.ID)
		if err != nil {
			return nil, false, err
		}
		revision := uint64(0)
		if old != nil {
			revision = old.Summary.Revision
		}
		info, err = InstallPlugin(ctx, db, info, revision, source.Summary.Enabled)
		if err != nil {
			return nil, false, err
		}
		if info.Summary.Enabled != source.Summary.Enabled {
			info, err = SetPluginEnabled(ctx, db, record.ID, info.Summary.Revision, source.Summary.Enabled)
			if err != nil {
				return nil, false, err
			}
		}
		status, err := ReadConfigurationFollow(ctx, db)
		if err != nil {
			return nil, false, err
		}
		if status.Source != nil {
			info.Summary.Source = &protocol.ConfigurationOrigin{Node: *status.Source, Revision: record.Revision}
			if err = savePluginInventory(ctx, db, info, true); err != nil {
				return nil, false, err
			}
		}
		return []protocol.Event{{Kind: "plugin_changed", Data: info.Summary}}, false, nil
	case "plugin_settings":
		var incoming pluginFollowSettings
		if err := decodeConfigurationRecord(record, &incoming); err != nil {
			return nil, false, err
		}
		if incoming.Package.Name != record.ID {
			return nil, false, invalid("plugin settings identity does not match manifest")
		}
		info, err := ReadPlugin(ctx, db, record.ID)
		if err != nil {
			return nil, false, err
		}
		if info == nil || info.Summary.Digest != incoming.Package.Digest {
			return nil, false, fault(protocol.ErrorCodeNotConfigured, "matching plugin version is pending")
		}
		incoming.Package = plugins.PackageReference(*info)
		if _, _, err := followedPackage(ctx, db, incoming.Package); err != nil {
			return nil, false, err
		}
		old, err := readStoredPluginSettings(ctx, db, *info)
		if err != nil {
			return nil, false, err
		}
		if len(info.Mcp) > 0 {
			if err := validateMCPConfiguration(*info, incoming.MCP); err != nil {
				return nil, false, err
			}
		}
		slots := pluginFollowSecrets{Slots: map[string]protocol.Secret{}}
		if record.Secret != nil {
			if err := json.Unmarshal([]byte(record.Secret.Expose()), &slots); err != nil {
				return nil, false, invalid("plugin secret payload is invalid")
			}
		}
		updates := map[string]protocol.PluginSettingsSecretUpdate{}
		targets := MCPSettingsTargets(info.Settings, incoming.MCP)
		if info.Settings != nil {
			for name, field := range info.Settings.Properties {
				if field.Secret == nil {
					continue
				}
				update := protocol.PluginSettingsSecretUpdate{Kind: "clear"}
				if sameSettingsBinding(old, info.Settings, name, targets) {
					update.Kind = "keep"
				}
				if key, ok := slots.Slots[name]; ok {
					update = protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: key}
				}
				updates[name] = update
			}
			if _, err := SavePluginSettings(ctx, db, protocol.CommandSavePluginSettingsPayload{Package: plugins.PackageReference(*info), Values: incoming.Values, Secrets: updates}, targets); err != nil {
				return nil, false, err
			}
			info, err = ReadPlugin(ctx, db, record.ID)
			if err != nil {
				return nil, false, err
			}
			old, err = readStoredPluginSettings(ctx, db, *info)
			if err != nil {
				return nil, false, err
			}
		}
		// Preserve target-only rotating authorizations and append a host-local
		// settings revision; source revision numbers never index captured settings.
		old.McpAllowed = maps.Clone(incoming.Allowed)
		if len(info.Mcp) > 0 {
			old.Mcp, err = json.Marshal(incoming.MCP)
			if err != nil {
				return nil, false, err
			}
		}
		saved, err := persistMCPSettings(ctx, db, *info, old)
		if err != nil {
			return nil, false, err
		}
		stateMissing := false
		if saved.Settings != nil {
			state := projectPluginSettings(saved, old, targets)
			stateMissing = !state.Ready
		}
		return []protocol.Event{{Kind: "plugin_changed", Data: saved.Summary}}, stateMissing, nil
	case "connections":
		var incoming pluginFollowConnection
		if err := decodeConfigurationRecord(record, &incoming); err != nil {
			return nil, false, err
		}
		if incoming.ID.String() != record.ID {
			return nil, false, invalid("connection identity does not match manifest")
		}
		info, err := ReadPlugin(ctx, db, incoming.Package.Name)
		if err != nil {
			return nil, false, err
		}
		if info == nil || info.Summary.Digest != incoming.Package.Digest {
			return nil, false, fault(protocol.ErrorCodeNotConfigured, "connector plugin version is pending")
		}
		var declaration *protocol.PluginConnectorDeclaration
		for _, entry := range ConnectorDeclarations(*info) {
			if entry.ID == incoming.Connector {
				value := entry
				declaration = &value
				break
			}
		}
		if declaration == nil {
			return nil, false, fault(protocol.ErrorCodeNotConfigured, "connector declaration is pending")
		}
		if !slices.ContainsFunc(declaration.Auth, func(auth protocol.PluginConnectorAuth) bool { return auth.Kind == string(incoming.Method) }) {
			return nil, false, invalid("connector source authentication is not declared")
		}
		current, err := ReadConnectorRecord(ctx, db, incoming.ID)
		exists := err == nil
		var reported *protocol.Fault
		if err != nil && (!errors.As(err, &reported) || reported.Code != protocol.ErrorCodeNotFound) {
			return nil, false, err
		}
		if exists && (current.Plugin != incoming.Package.Name || current.Connector != incoming.Connector) {
			return nil, false, fault(protocol.ErrorCodeConflict, "connection identity belongs to another declaration")
		}
		if !exists {
			current = ConnectorRecord{ID: incoming.ID, Plugin: incoming.Package.Name, Connector: incoming.Connector, Secrets: map[string]protocol.Secret{}, GrantRevision: 1}
		}
		// A changed method or endpoint invalidates target authorization; identical
		// definitions retain their independent target login and refresh state.
		bound := connectorBound(*info, *declaration, incoming.Method)
		if current.Method != incoming.Method || !reflect.DeepEqual(current.Bound, bound) {
			current.Grant, current.Identity, current.Login = nil, nil, nil
			current.Secrets = map[string]protocol.Secret{}
			current.State = protocol.PluginConnectorStateNotConnected
		}
		current.Method, current.Enabled, current.Fields, current.Bound = incoming.Method, incoming.Enabled, map[string]string{}, bound
		secrets := map[string]protocol.Secret{}
		if record.Secret != nil {
			if incoming.Method != protocol.PluginConnectorKindSecret {
				return nil, false, invalid("rotating connector authorization cannot follow")
			}
			if err := json.Unmarshal([]byte(record.Secret.Expose()), &secrets); err != nil {
				return nil, false, invalid("connector static secret payload is invalid")
			}
		}
		if current.Secrets == nil {
			current.Secrets = map[string]protocol.Secret{}
		}
		if err := connectorFields(&current, protocol.PluginConnectorInput{Fields: incoming.Fields, Secrets: secrets}, *declaration); err != nil {
			return nil, false, err
		}
		current.Revision, err = nextRevision(current.Revision)
		if err != nil {
			return nil, false, err
		}
		current.AuthRevision = current.Revision
		missing := true
		if current.Method == protocol.PluginConnectorKindSecret {
			missing = len(current.Secrets) == 0
			if !missing {
				current.State = protocol.PluginConnectorStateConnected
			}
		} else if current.Grant != nil {
			missing = false
		}
		if err := saveConnectorRecord(ctx, db, current); err != nil {
			return nil, false, err
		}
		return []protocol.Event{{Kind: "plugin_changed", Data: info.Summary}}, missing, nil
	}
	return nil, false, invalid("unsupported plugin follow category")
}
func removePluginFollowCopy(ctx context.Context, db *sql.Conn, node protocol.NodeID, category, id string) ([]protocol.Event, error) {
	switch category {
	case "plugins":
		info, err := ReadPlugin(ctx, db, id)
		if err != nil || info == nil {
			return nil, err
		}
		_, err = RemovePlugin(ctx, db, id, info.Summary.Revision)
		return []protocol.Event{{Kind: "plugin_removed", Data: protocol.EventPluginRemovedPayload{Name: id}}}, err
	case "plugin_settings":
		return nil, nil // Captured settings remain immutable and are collected by existing retention.
	case "connections":
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return nil, err
		}
		_, err = protocol.ParseRequestID(id)
		if err != nil {
			return nil, err
		}
		_, err = db.ExecContext(ctx, "DELETE FROM plugin_connections WHERE id=?1 AND user=?2", id, user.ID.String())
		return nil, err
	}
	return nil, invalid("unsupported plugin follow removal")
}

// CheckConfigurationMCPWritable applies the follow lock at the changed server
// boundary. A local overlay never unlocks the rest of a followed package.
func CheckConfigurationMCPWritable(ctx context.Context, db *sql.Conn, plugin string, servers []string) error {
	if configurationApplying(ctx) {
		return nil
	}
	locked := CheckConfigurationEditable(ctx, db, "plugin_settings", plugin)
	if locked == nil {
		return nil
	}
	var reported *protocol.Fault
	if !errors.As(locked, &reported) || reported.Code != protocol.ErrorCodePermissionDenied {
		return locked
	}
	for _, server := range servers {
		local, err := ConfigurationMCPServerLocal(ctx, db, plugin, server)
		if err != nil {
			return err
		}
		if !local {
			return locked
		}
	}
	return nil
}
