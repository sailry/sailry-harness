package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// configurationCacheUser distinguishes current controller-owned replicas from
// legacy materialized records retained for already captured work.
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

func listCachedConfiguration[T any](ctx context.Context, db *sql.Conn, category string) ([]T, error) {
	user, _, err := configurationCacheUser(ctx, db)
	if err != nil {
		return nil, err
	}
	return readBodies[T](ctx, db, "SELECT body FROM configuration_cache WHERE user=?1 AND category=?2 ORDER BY id", user.ID.String(), category)
}

func cacheConfigurationRecord(ctx context.Context, db *sql.Conn, user protocol.User, category string, record protocol.ConfigurationFollowRecord) error {
	var value any
	if err := decodeConfigurationRecord(record, &value); err != nil {
		return err
	}
	if kind, ok := configurationKind(category); ok && kind.Validate != nil {
		if err := kind.Validate(ctx, db, record); err != nil {
			return err
		}
	}
	var secret any
	if record.Secret != nil {
		secret = record.Secret.Expose()
	}
	var previous string
	err := db.QueryRowContext(ctx, "SELECT digest FROM configuration_cache_history WHERE user=?1 AND category=?2 AND id=?3 AND revision=?4", user.ID.String(), category, record.ID, record.Revision).Scan(&previous)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	if err == nil && previous != record.Digest {
		return fault(protocol.ErrorCodeRevisionConflict, "configuration revision was reused for different content")
	}
	if _, err := db.ExecContext(ctx, `INSERT INTO configuration_cache_history(user,category,id,revision,digest,body,secret,expires_at_ms)
VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(user,category,id,revision) DO NOTHING`,
		user.ID.String(), category, record.ID, record.Revision, record.Digest, []byte(record.Body), secret, record.ExpiresAtMs); err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, `INSERT INTO configuration_cache(user,category,id,revision,digest,body,secret,expires_at_ms)
VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(user,category,id) DO UPDATE SET
revision=excluded.revision,digest=excluded.digest,body=excluded.body,secret=excluded.secret,expires_at_ms=excluded.expires_at_ms`,
		user.ID.String(), category, record.ID, record.Revision, record.Digest, []byte(record.Body), secret, record.ExpiresAtMs)
	return err
}

func applyConfigurationCache(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, manifest protocol.ConfigurationManifest) ([]protocol.Event, error) {
	user, cached, err := configurationCacheUser(ctx, db)
	if err != nil {
		return nil, err
	}
	if !cached || user.Controller == nil || *user.Controller != caller || manifest.Source != caller || caller == node {
		return nil, fault(protocol.ErrorCodePermissionDenied, "configuration does not belong to this controller")
	}
	digest, err := configurationManifestDigest(manifest.Records)
	if err != nil {
		return nil, err
	}
	var revision uint64
	var previousDigest string
	err = db.QueryRowContext(ctx, "SELECT revision,digest FROM configuration_follow_versions WHERE user=?1 AND category=?2 AND source=?3", user.ID.String(), manifest.Category, caller[:]).Scan(&revision, &previousDigest)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return nil, err
	}
	if manifest.Revision < revision || manifest.Revision == revision && digest != previousDigest {
		return nil, fault(protocol.ErrorCodeRevisionConflict, "configuration manifest revision changed")
	}
	seen := map[string]bool{}
	for _, record := range manifest.Records {
		if len(record.Body) == 0 {
			previous, err := readConfigurationCacheRecord(ctx, db, manifest.Category, record.ID, 0)
			if err != nil {
				return nil, err
			}
			if previous == nil || previous.Digest != record.Digest || previous.Revision != record.Revision {
				return nil, fault(protocol.ErrorCodeRevisionConflict, "configuration record content is missing")
			}
			record = *previous
		}
		if err := cacheConfigurationRecord(ctx, db, user, manifest.Category, record); err != nil {
			return nil, err
		}
		seen[record.ID] = true
	}
	// Event hooks receive the committed content even when delivery omitted
	// an unchanged body that the receiver already had.
	for index := range manifest.Records {
		if len(manifest.Records[index].Body) == 0 {
			cached, err := readConfigurationCacheRecord(ctx, db, manifest.Category, manifest.Records[index].ID, 0)
			if err != nil || cached == nil {
				return nil, err
			}
			manifest.Records[index] = *cached
		}
	}
	rows, err := db.QueryContext(ctx, "SELECT id FROM configuration_cache WHERE user=?1 AND category=?2", user.ID.String(), manifest.Category)
	if err != nil {
		return nil, err
	}
	var removed []string
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			rows.Close()
			return nil, err
		}
		if !seen[id] {
			removed = append(removed, id)
		}
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return nil, err
	}
	for _, id := range removed {
		if _, err := db.ExecContext(ctx, "DELETE FROM configuration_cache WHERE user=?1 AND category=?2 AND id=?3", user.ID.String(), manifest.Category, id); err != nil {
			return nil, err
		}
	}
	if _, err := db.ExecContext(ctx, `INSERT INTO configuration_follow_versions(user,category,source,revision,digest) VALUES(?1,?2,?3,?4,?5)
ON CONFLICT(user,category,source) DO UPDATE SET revision=excluded.revision,digest=excluded.digest`, user.ID.String(), manifest.Category, caller[:], manifest.Revision, digest); err != nil {
		return nil, err
	}
	if _, err := db.ExecContext(ctx, `INSERT INTO configuration_follow_sources(user,source,revision) VALUES(?1,?2,1)
ON CONFLICT(user) DO UPDATE SET source=excluded.source,revision=revision+1 WHERE source IS NULL`, user.ID.String(), caller[:]); err != nil {
		return nil, err
	}
	status, err := ReadConfigurationFollow(ctx, db)
	if err != nil {
		return nil, err
	}
	changes, err := configurationCacheEvents(ctx, db, manifest, removed)
	if err != nil {
		return nil, err
	}
	return append(changes, protocol.Event{Kind: "configuration_follow_changed", Data: status}), nil
}

func configurationCacheEvents(ctx context.Context, db *sql.Conn, manifest protocol.ConfigurationManifest, removed []string) ([]protocol.Event, error) {
	kind, ok := configurationKind(manifest.Category)
	if !ok {
		return nil, invalid("unsupported configuration category")
	}
	changes := []protocol.Event{}
	if kind.Changed != nil {
		current, err := kind.Changed(ctx, db, manifest.Records)
		if err != nil {
			return nil, err
		}
		changes = append(changes, current...)
	}
	if kind.Removed != nil {
		for _, id := range removed {
			change, err := kind.Removed(id)
			if err != nil {
				return nil, err
			}
			if change != nil {
				changes = append(changes, *change)
			}
		}
	}
	return changes, nil
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
