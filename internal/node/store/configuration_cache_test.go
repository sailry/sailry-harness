package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
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
	t.Parallel()
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

func TestRuntimeCredentialEvent(t *testing.T) {
	t.Parallel()
	for _, event := range []string{"provider_changed", "provider_removed", "credential_changed", "connections_changed", "plugin_changed", "plugin_removed"} {
		if !RuntimeCredentialEvent(event) {
			t.Errorf("%s did not wake credential listeners", event)
		}
	}
	if RuntimeCredentialEvent("role_changed") || RuntimeCredentialEvent("unregistered_changed") {
		t.Error("unrelated configuration event woke credential listeners")
	}
}

func TestConfigurationCacheKeepsCapturedRuntimeCredentialsAndVersions(t *testing.T) {
	t.Parallel()
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
		if err := seedHistoricalRuntimeCredentials(ctx, db, credential); err != nil {
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
		if err := seedHistoricalPluginPackage(ctx, db, info, declared); err != nil {
			return err
		}
		capture := plugins.PackageReference(info)
		old := pluginFollowSettings{Package: capture, Values: map[string]json.RawMessage{"choice": json.RawMessage(`"old"`)}, MCP: declared}
		if err := seedHistoricalConfiguration(ctx, db, "plugin_settings", pluginSettingsCacheID(capture), 1, old); err != nil {
			return err
		}
		info.Summary.Digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
		if err := seedHistoricalPluginPackage(ctx, db, info, declared); err != nil {
			return err
		}
		current := pluginFollowSettings{Package: plugins.PackageReference(info), Values: map[string]json.RawMessage{"choice": json.RawMessage(`"new"`)}, MCP: declared}
		if err := seedHistoricalConfiguration(ctx, db, "plugin_settings", pluginSettingsCacheID(current.Package), 1, current); err != nil {
			return err
		}
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM configuration_cache WHERE user=?1 AND category='plugin_settings' AND id=?2", user.ID.String(), pluginSettingsCacheID(capture)); err != nil {
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
	t.Parallel()
	storage, _, _, ctx := controllerCacheFixture(t)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if value, err := readPersonalSetting[protocol.MediaSettings](ctx, db, "media"); err != nil || value != nil {
			return fmt.Errorf("remote media inherited local settings: %v", err)
		}
		media := protocol.DefaultMediaSettings()
		media.Revision = 7
		if err := seedHistoricalConfiguration(ctx, db, "personal:media", "default", 8, media); err != nil {
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
		var historical protocol.MediaSettings
		if err != nil || retained == nil || json.Unmarshal(retained.Body, &historical) != nil || historical.Revision != 7 {
			return fmt.Errorf("the historical media settings record was lost: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
