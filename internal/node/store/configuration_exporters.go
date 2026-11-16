package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"maps"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func exportProviderConfiguration(ctx context.Context, db *sql.Conn, _ ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error) {
	values, err := providerConfigurations(ctx, db)
	if err != nil {
		return nil, err
	}
	result := []protocol.ConfigurationFollowRecord{}
	for _, saved := range values {
		if !saved.Listed || saved.Provider == nil {
			continue
		}
		value := *saved.Provider
		value.Owner, value.Shared, value.Source, value.Consent = nil, false, nil, nil
		record, err := configurationRecord(value.ID.String(), value.Revision, value, nil)
		if err != nil {
			return nil, err
		}
		result = append(result, record)
	}
	return result, nil
}

func exportRoleConfiguration(ctx context.Context, db *sql.Conn, _ ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error) {
	values, err := ReadRoles(ctx, db)
	if err != nil {
		return nil, err
	}
	result := []protocol.ConfigurationFollowRecord{}
	for _, value := range values {
		value.Source = nil
		record, err := configurationRecord(value.ID.String(), value.Revision, value, nil)
		if err != nil {
			return nil, err
		}
		result = append(result, record)
	}
	return result, nil
}

func exportTeamConfiguration(ctx context.Context, db *sql.Conn, _ ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error) {
	values, err := ReadTeams(ctx, db)
	if err != nil {
		return nil, err
	}
	result := []protocol.ConfigurationFollowRecord{}
	for _, value := range values {
		value.Source = nil
		record, err := configurationRecord(value.ID.String(), value.Revision, value, nil)
		if err != nil {
			return nil, err
		}
		result = append(result, record)
	}
	return result, nil
}

func exportSshConfiguration(ctx context.Context, db *sql.Conn, options ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error) {
	values, err := ListSsh(ctx, db)
	if err != nil {
		return nil, err
	}
	result := []protocol.ConfigurationFollowRecord{}
	for _, value := range values {
		value.Source, value.HostKey = nil, nil
		var secret *protocol.Secret
		if options.Secrets {
			_, credential, err := readSshRecord(ctx, db, value.ID, value.Revision)
			if err != nil {
				return nil, err
			}
			body, err := json.Marshal(credential)
			if err != nil {
				return nil, err
			}
			secret = pointerSecret(string(body))
		}
		record, err := configurationRecord(value.ID.String(), value.Revision, value, secret)
		if err != nil {
			return nil, err
		}
		result = append(result, record)
	}
	return result, nil
}

func exportDatabaseConfiguration(ctx context.Context, db *sql.Conn, options ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error) {
	values, err := ListDatabases(ctx, db)
	if err != nil {
		return nil, err
	}
	result := []protocol.ConfigurationFollowRecord{}
	for _, value := range values {
		value.Source = nil
		var secret *protocol.Secret
		if options.Secrets && value.Connection.Kind != "sqlite" {
			password, err := readDatabasePassword(ctx, db, value.ID)
			if err != nil {
				return nil, err
			}
			secret = &password
		}
		record, err := configurationRecord(value.ID.String(), value.Revision, value, secret)
		if err != nil {
			return nil, err
		}
		result = append(result, record)
	}
	return result, nil
}

func exportDefaultsConfiguration(ctx context.Context, db *sql.Conn, options ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error) {
	value, err := ReadDefaults(ctx, db)
	if err != nil || value.Config == nil {
		return nil, err
	}
	value.Source = nil
	config := *value.Config
	config.Resource = nil
	if !options.Secrets {
		config.Credential = nil
	}
	value.Config = &config
	record, err := configurationRecord("default", value.Revision, value, nil)
	return []protocol.ConfigurationFollowRecord{record}, err
}

func exportTeamSettingsConfiguration(ctx context.Context, db *sql.Conn, _ ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error) {
	value, err := ReadTeamSettings(ctx, db)
	if err != nil {
		return nil, err
	}
	value.Source = nil
	record, err := configurationRecord("default", value.Revision, value, nil)
	return []protocol.ConfigurationFollowRecord{record}, err
}

func exportPluginConfiguration(ctx context.Context, db *sql.Conn, _ ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error) {
	values, err := ReadPluginInfos(ctx, db)
	if err != nil {
		return nil, err
	}
	result := []protocol.ConfigurationFollowRecord{}
	for _, value := range values {
		value.Summary.Source = nil
		record, err := configurationRecord(value.Summary.Name, value.Summary.Revision, value, nil)
		if err != nil {
			return nil, err
		}
		result = append(result, record)
	}
	return result, nil
}

func exportPluginSettingsConfiguration(ctx context.Context, db *sql.Conn, options ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error) {
	infos, err := ReadPluginInfos(ctx, db)
	if err != nil {
		return nil, err
	}
	result := []protocol.ConfigurationFollowRecord{}
	for _, info := range infos {
		settings, err := ResolvePluginSettings(ctx, db, plugins.PackageReference(info))
		if err != nil {
			return nil, err
		}
		mcp := options.Declarations[info.Summary.Name]
		if len(info.Mcp) != 0 {
			mcp, err = EffectivePluginMCP(settings, mcp)
			if err != nil {
				return nil, err
			}
		}
		value := pluginFollowSettings{Package: plugins.PackageReference(info), Values: settings.Values, MCP: mcp, Allowed: settings.MCPAllowed}
		captured, err := readStoredPluginSettings(ctx, db, info)
		if err != nil {
			return nil, err
		}
		if len(captured.Mcp) != 0 && string(captured.Mcp) != "null" {
			if err := json.Unmarshal(captured.Mcp, &value.MCP); err != nil {
				return nil, err
			}
		}
		var secret *protocol.Secret
		if options.Secrets && len(settings.Secrets) != 0 {
			slots := maps.Clone(settings.Secrets)
			if info.Extension != nil {
				for _, authentication := range info.Extension.ModelAuth {
					if name := authentication.Client.ClientSecretSetting; name != nil {
						delete(slots, *name)
					}
				}
			}
			body, err := json.Marshal(pluginFollowSecrets{Slots: slots})
			if err != nil {
				return nil, err
			}
			secret = pointerSecret(string(body))
		}
		revision, err := nextRevision(info.Summary.SettingsRevision)
		if err != nil {
			return nil, err
		}
		record, err := configurationRecord(pluginSettingsCacheID(value.Package), revision, value, secret)
		if err != nil {
			return nil, err
		}
		result = append(result, record)
	}
	return result, nil
}

func exportConnectionConfiguration(ctx context.Context, db *sql.Conn, _ ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error) {
	records, err := connectorRecords(ctx, db)
	if err != nil {
		return nil, err
	}
	result := []protocol.ConfigurationFollowRecord{}
	for _, record := range records {
		info, err := ReadPlugin(ctx, db, record.Plugin)
		if err != nil {
			return nil, err
		}
		if info == nil {
			continue
		}
		value := pluginFollowConnection{
			Package: plugins.PackageReference(*info), ID: record.ID, Connector: record.Connector, Method: record.Method,
			Enabled: record.Enabled, Fields: maps.Clone(record.Fields), Revision: record.Revision, AuthRevision: record.AuthRevision,
			GrantRevision: record.GrantRevision, Identity: record.Identity, Bound: record.Bound, State: record.State,
			CheckedAtMs: record.CheckedAtMs, Login: record.Login,
		}
		for name := range value.Fields {
			if !providers.RuntimeField(name) {
				delete(value.Fields, name)
			}
		}
		entry, err := configurationRecord(record.ID.String(), record.Revision, value, nil)
		if err != nil {
			return nil, err
		}
		result = append(result, entry)
	}
	return result, nil
}
