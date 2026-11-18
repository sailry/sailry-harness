package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// configurationCacheUser confines historical cached records to the paired user
// whose work captured them.
func configurationCacheUser(ctx context.Context, db *sql.Conn) (protocol.User, bool, error) {
	user, err := CurrentUser(ctx, db)
	return user, err == nil && !user.Local && user.Controller != nil, err
}

func ReadConfigurationCache(ctx context.Context, db *sql.Conn, category, id string) (json.RawMessage, bool, error) {
	user, cached, err := configurationCacheUser(ctx, db)
	if err != nil || !cached {
		return nil, false, err
	}
	var body []byte
	err = db.QueryRowContext(ctx, "SELECT body FROM configuration_cache WHERE user=?1 AND category=?2 AND id=?3", user.ID.String(), category, id).Scan(&body)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, false, nil
	}
	return body, err == nil, err
}

func readConfigurationCacheRecord(ctx context.Context, db *sql.Conn, category, id string, revision uint64) (*protocol.ConfigurationFollowRecord, error) {
	user, cached, err := configurationCacheUser(ctx, db)
	if err != nil || !cached {
		return nil, err
	}
	value := protocol.ConfigurationFollowRecord{ID: id}
	var secret sql.NullString
	var expiry sql.NullInt64
	table, condition, args := "configuration_cache", "", []any{user.ID.String(), category, id}
	if revision != 0 {
		table, condition = "configuration_cache_history", " AND revision=?4"
		args = append(args, revision)
	}
	err = db.QueryRowContext(ctx, "SELECT revision,digest,body,secret,expires_at_ms FROM "+table+" WHERE user=?1 AND category=?2 AND id=?3"+condition, args...).Scan(&value.Revision, &value.Digest, &value.Body, &secret, &expiry)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	if secret.Valid {
		value.Secret = pointerSecret(secret.String)
	}
	if expiry.Valid {
		value.ExpiresAtMs = new(uint64)
		*value.ExpiresAtMs = uint64(expiry.Int64)
	}
	return &value, nil
}

func pointerSecret(value string) *protocol.Secret {
	secret := protocol.NewSecret(value)
	return &secret
}

func readCachedConfiguration[T any](ctx context.Context, db *sql.Conn, category, id string) (*T, error) {
	body, found, err := ReadConfigurationCache(ctx, db, category, id)
	if err != nil || !found {
		return nil, err
	}
	var value T
	if err := json.Unmarshal(body, &value); err != nil {
		return nil, err
	}
	return &value, nil
}

func pluginSettingsCacheID(reference protocol.PluginReference) string {
	return reference.Name + ":" + reference.Digest
}

func cachedPluginSettings(ctx context.Context, db *sql.Conn, reference protocol.PluginReference) (storedPluginSettings, error) {
	// Settings revision zero is a real default capture; wire record revisions
	// are positive, so the immutable cache uses revision + 1.
	cachedRevision, err := nextRevision(reference.SettingsRevision)
	if err != nil {
		return storedPluginSettings{}, err
	}
	record, err := readConfigurationCacheRecord(ctx, db, "plugin_settings", pluginSettingsCacheID(reference), cachedRevision)
	if err != nil {
		return storedPluginSettings{}, err
	}
	if record == nil {
		return storedPluginSettings{}, fault(protocol.ErrorCodeNotConfigured, "captured plugin settings are unavailable")
	}
	var value pluginFollowSettings
	if err := json.Unmarshal(record.Body, &value); err != nil {
		return storedPluginSettings{}, err
	}
	info, _, err := followedPackage(ctx, db, value.Package)
	if err != nil {
		return storedPluginSettings{}, err
	}
	if value.Package != reference {
		return storedPluginSettings{}, invalid("cached plugin settings identity differs from its capture")
	}
	if info.Settings != nil {
		if err := plugins.ValidatePublicSettings(*info.Settings, value.Values); err != nil {
			return storedPluginSettings{}, err
		}
	}
	result := storedPluginSettings{
		Schema: info.Settings, Values: value.Values, Slots: map[string]*protocol.Secret{},
		McpTargets: MCPSettingsTargets(info.Settings, value.MCP), McpAllowed: value.Allowed,
	}
	declared := value.MCP
	if len(declared.Servers) == 0 {
		_, declared, err = followedPackage(ctx, db, reference)
		if err != nil {
			return result, err
		}
	}
	result.Mcp, err = json.Marshal(declared)
	if err != nil {
		return result, err
	}
	if record.Secret != nil {
		var secrets pluginFollowSecrets
		if err := json.Unmarshal([]byte(record.Secret.Expose()), &secrets); err != nil {
			return result, err
		}
		for key, value := range secrets.Slots {
			if settingsBinding(info.Settings, key) == nil {
				return result, invalid("cached secret is not declared by the package")
			}
			result.Slots[key] = pointerSecret(value.Expose())
		}
	}
	return result, nil
}

func cachedProviderConfiguration(ctx context.Context, db *sql.Conn, id protocol.ProviderID) (*providerConfiguration, error) {
	record, err := readConfigurationCacheRecord(ctx, db, "providers", id.String(), 0)
	if err != nil || record == nil {
		return nil, err
	}
	var provider protocol.ConversationProvider
	if err := json.Unmarshal(record.Body, &provider); err != nil {
		return nil, err
	}
	user, _, err := configurationCacheUser(ctx, db)
	if err != nil {
		return nil, err
	}
	provider.Owner, provider.Shared = &user.ID, false
	provider.Source = &protocol.ConfigurationOrigin{Node: *user.Controller, Revision: record.Revision}
	saved := &providerConfiguration{ID: provider.ID, Listed: true, Provider: &provider, Authorizations: map[protocol.CredentialID]providerAuthorization{}}
	if provider.Credential != nil {
		body, found, err := ReadConfigurationCache(ctx, db, "runtime_credentials", "provider:"+provider.Credential.ID.String())
		if err != nil {
			return nil, err
		}
		if found {
			var runtime protocol.RuntimeCredential
			if err := json.Unmarshal(body, &runtime); err != nil {
				return nil, err
			}
			var key *protocol.Secret
			if runtime.State == "ready" || runtime.State == "uncertain" {
				value := runtime.Access
				key = &value
			}
			saved.Authorizations[provider.Credential.ID] = providerAuthorization{
				Authentication: provider.Authentication, Revision: runtime.Revision, Key: key, ExpiresAtMs: runtime.ExpiresAtMs,
			}
		}
	}
	return saved, nil
}
