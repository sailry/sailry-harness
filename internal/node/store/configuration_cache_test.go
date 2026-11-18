package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"slices"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func controllerCacheFixture(t *testing.T) (*Store, protocol.NodeID, protocol.NodeID, context.Context) {
	t.Helper()
	_, node, storage := schemaFixture(t)
	controller := protocol.NodeID{61}
	var user protocol.User
	err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
		var err error
		user, err = CreateUser(t.Context(), db, "Cache controller")
		if err != nil {
			return err
		}
		_, err = db.ExecContext(t.Context(), "UPDATE users SET controller=?2 WHERE id=?1", user.ID.String(), controller[:])
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	user.Controller = &controller
	return storage, node, controller, WithUser(t.Context(), user)
}

func TestConfigurationCacheStatusIgnoresLegacyCopyConflicts(t *testing.T) {
	storage, _, controller, ctx := controllerCacheFixture(t)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		old := protocol.NodeID{62}
		if _, err := db.ExecContext(ctx, "INSERT INTO configuration_follow_sources(user,source,revision) VALUES(?1,?2,7)", user.ID.String(), old[:]); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, `INSERT INTO configuration_follow_records(user,category,id,source,revision,digest,state,message,body,materialized)
VALUES(?1,'providers','old',?2,1,'old','conflict','legacy mirror conflict','{}',0)`, user.ID.String(), old[:]); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO configuration_follow_mcp_overrides(user,plugin,server) VALUES(?1,'old','server')", user.ID.String()); err != nil {
			return err
		}
		status, err := ReadConfigurationFollow(ctx, db)
		if err != nil {
			return err
		}
		if status.Source == nil || *status.Source != controller || len(status.Issues) != 0 || len(status.MCPLocal) != 0 {
			return fmt.Errorf("cache status retained obsolete copy controls: %+v", status)
		}
		var retained int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM configuration_follow_records WHERE user=?1", user.ID.String()).Scan(&retained); err != nil {
			return err
		}
		if retained != 1 {
			return fmt.Errorf("status read removed historical configuration")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestConfigurationRegistryOwnsCategoryAndEvents(t *testing.T) {
	seen := map[string]bool{}
	storage, _, _, ctx := controllerCacheFixture(t)
	for _, name := range ConfigurationCategories {
		kind, exists := configurationKind(name)
		if !exists || kind.Export == nil || seen[name] {
			t.Fatalf("incomplete or duplicate registered category %s", name)
		}
		seen[name] = true
		for _, event := range kind.Events {
			if !ConfigurationChanged(event) || !slices.Contains(configurationRegistry.events[event], name) {
				t.Fatalf("category %s event %s has no dirty routing", name, event)
			}
		}
		if kind.Changed != nil {
			if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
				events, err := kind.Changed(ctx, db, nil)
				if err != nil {
					return err
				}
				for _, event := range events {
					body, err := json.Marshal(event)
					if err != nil {
						return fmt.Errorf("%s event %s: %w", name, event.Kind, err)
					}
					var decoded protocol.Event
					if err := json.Unmarshal(body, &decoded); err != nil {
						return fmt.Errorf("%s event %s decode: %w", name, event.Kind, err)
					}
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
		}
	}
	if ConfigurationChanged("unregistered_changed") || RuntimeCredentialEvent("role_changed") ||
		!RuntimeCredentialEvent("provider_changed") || !RuntimeCredentialEvent("provider_removed") ||
		!RuntimeCredentialEvent("credential_changed") || !RuntimeCredentialEvent("connections_changed") {
		t.Fatal("configuration event routing inferred names or missed runtime metadata")
	}
	for _, dependency := range [][2]string{
		{"plugins", "plugin_settings"}, {"plugin_settings", "runtime_credentials"},
		{"runtime_credentials", "roles"}, {"roles", "teams"}, {"ssh_profiles", "database_profiles"},
	} {
		if slices.Index(ConfigurationCategories, dependency[0]) >= slices.Index(ConfigurationCategories, dependency[1]) {
			t.Fatalf("configuration dependency order reversed: %v", dependency)
		}
	}
}

func TestConfigurationCacheKeepsCapturedRuntimeCredentialsAndVersions(t *testing.T) {
	storage, node, controller, ctx := controllerCacheFixture(t)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		provider := providerFixture()
		provider.Revision = 1
		provider.Credential = &protocol.CredentialRef{Node: controller, ID: protocol.NewCredentialID()}
		expiry := uint64(time.Now().Add(time.Hour).UnixMilli())
		credential := protocol.RuntimeCredential{
			Controller: controller, Kind: "provider", ID: provider.Credential.ID.String(), Generation: provider.Credential.ID.String(),
			Revision: 3, State: "ready", Provider: &provider.ID, Authentication: protocol.AuthenticationApiKey,
			Access: protocol.NewSecret("cache-api-key"), ExpiresAtMs: &expiry,
		}
		if _, err := applyConfigurationCache(ctx, db, node, controller, protocol.ConfigurationManifest{
			Source: controller, Category: "runtime_credentials", Revision: 1,
			Records: []protocol.ConfigurationFollowRecord{followRecord(t, credential.CacheID(), credential.Revision, credential, nil)},
		}); err != nil {
			return err
		}
		saved, err := readProviderConfiguration(ctx, db, provider.ID)
		if err != nil || saved != nil {
			return fmt.Errorf("runtime cache became a model channel: %v", err)
		}
		key, err := ReadRuntimeProviderCredential(ctx, db, node, provider, *provider.Credential)
		if err != nil || key.Expose() != "cache-api-key" {
			return fmt.Errorf("cache did not resolve captured execution key: %w", err)
		}
		info := pluginTestInfo("history-cache", 'a')
		schema, err := plugins.ValidateSettings([]byte(`{"$schema":"` + plugins.SettingsSchema + `","type":"object","additionalProperties":false,"properties":{"choice":{"type":"string"}}}`))
		if err != nil {
			return err
		}
		info.Settings = &schema
		declared := protocol.PluginMcpConfiguration{Servers: map[string]protocol.PluginMcpServer{}}
		if err := CacheConfigurationPackage(ctx, db, info, declared); err != nil {
			return err
		}
		capture := plugins.PackageReference(info)
		old := pluginFollowSettings{Package: capture, Values: map[string]json.RawMessage{"choice": json.RawMessage(`"old"`)}, MCP: declared}
		oldRecord := followRecord(t, pluginSettingsCacheID(capture), 1, old, nil)
		if _, err := applyConfigurationCache(ctx, db, node, controller, protocol.ConfigurationManifest{
			Source: controller, Category: "plugin_settings", Revision: 1, Records: []protocol.ConfigurationFollowRecord{oldRecord},
		}); err != nil {
			return err
		}
		info.Summary.Digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
		if err := CacheConfigurationPackage(ctx, db, info, declared); err != nil {
			return err
		}
		current := pluginFollowSettings{Package: plugins.PackageReference(info), Values: map[string]json.RawMessage{"choice": json.RawMessage(`"new"`)}, MCP: declared}
		currentRecord := followRecord(t, pluginSettingsCacheID(current.Package), 1, current, nil)
		if _, err := applyConfigurationCache(ctx, db, node, controller, protocol.ConfigurationManifest{
			Source: controller, Category: "plugin_settings", Revision: 2, Records: []protocol.ConfigurationFollowRecord{currentRecord},
		}); err != nil {
			return err
		}
		oldSettings, err := cachedPluginSettings(ctx, db, capture)
		if err != nil || string(oldSettings.Values["choice"]) != `"old"` {
			return fmt.Errorf("package update replaced captured zero-revision settings: %w", err)
		}
		newSettings, err := cachedPluginSettings(ctx, db, current.Package)
		if err != nil || string(newSettings.Values["choice"]) != `"new"` {
			return fmt.Errorf("new package did not receive its own settings: %w", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestPersonalConfigurationUsesNodeLocalRowsForPairedUser(t *testing.T) {
	storage, node, controller, ctx := controllerCacheFixture(t)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if value, err := readPersonalSetting[protocol.MediaSettings](ctx, db, "media"); err != nil || value != nil {
			return fmt.Errorf("remote media inherited local settings: %v", err)
		}
		media := protocol.DefaultMediaSettings()
		media.Revision = 7
		record := followRecord(t, "default", 8, media, nil)
		_, err := applyConfigurationCache(ctx, db, node, controller, protocol.ConfigurationManifest{
			Source: controller, Category: "personal:media", Revision: 1, Records: []protocol.ConfigurationFollowRecord{record},
		})
		if err != nil {
			return err
		}
		value, err := readPersonalSetting[protocol.MediaSettings](ctx, db, "media")
		if err != nil || value != nil {
			return fmt.Errorf("the paired user inherited controller media settings: %v", err)
		}
		media.Revision = 1
		body, err := encode(media)
		if err != nil {
			return err
		}
		if err := writePersonalSetting(ctx, db, "media", body); err != nil {
			return err
		}
		value, err = readPersonalSetting[protocol.MediaSettings](ctx, db, "media")
		if err != nil || value == nil || value.Revision != 1 {
			return fmt.Errorf("the paired user's settings were not saved on the Node: %v", err)
		}
		retained, err := readConfigurationCacheRecord(ctx, db, "personal:media", "default", 8)
		if err != nil || retained == nil || retained.Digest != record.Digest {
			return fmt.Errorf("the old media settings record was lost: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
