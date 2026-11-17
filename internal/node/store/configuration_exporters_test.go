package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCompositeConfigurationExportsKeepImmutableRevisions(t *testing.T) {
	storage, _, _, cacheCtx := controllerCacheFixture(t)
	ctx := t.Context()
	name := "composite-export"
	connection := ConnectorRecord{
		ID: protocol.NewRequestID(), Plugin: name, Connector: "account", Revision: 1,
		Method: protocol.PluginConnectorKindSecret, State: protocol.PluginConnectorStateNotConnected,
		Fields: map[string]string{},
	}
	var previous map[string]protocol.ConfigurationFollowRecord
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo(name, 'a')
		if _, err := InstallPlugin(ctx, db, info, 0, true); err != nil {
			return err
		}
		return saveConnectorRecord(ctx, db, connection)
	}); err != nil {
		t.Fatal(err)
	}
	for _, change := range []string{"initial", "unchanged", "settings", "upgrade", "unchanged"} {
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			switch change {
			case "settings":
				if _, err := db.ExecContext(ctx, "UPDATE plugin_user_installations SET settings_revision=settings_revision+1 WHERE name=?1", name); err != nil {
					return err
				}
			case "upgrade":
				info, err := ReadPlugin(ctx, db, name)
				if err != nil {
					return err
				}
				replacement := pluginTestInfo(name, 'b')
				if _, err := InstallPlugin(ctx, db, replacement, info.Summary.Revision, true); err != nil {
					return err
				}
			}
			current := map[string]protocol.ConfigurationFollowRecord{}
			for category, export := range map[string]func() ([]protocol.ConfigurationFollowRecord, error){
				"plugins": func() ([]protocol.ConfigurationFollowRecord, error) {
					return exportPluginConfiguration(ctx, db, ConfigurationExport{})
				},
				"connections": func() ([]protocol.ConfigurationFollowRecord, error) {
					return exportConnectionConfiguration(ctx, db, ConfigurationExport{})
				},
			} {
				records, err := export()
				if err != nil {
					return err
				}
				if len(records) != 1 {
					return fmt.Errorf("%s: got %d records", category, len(records))
				}
				record := records[0]
				if record.Revision <= connection.Revision {
					return fmt.Errorf("%s: new export collides with legacy business revision", category)
				}
				if old, ok := previous[category]; ok {
					if change == "unchanged" && (old.Revision != record.Revision || old.Digest != record.Digest) {
						return fmt.Errorf("%s: unchanged export advanced", category)
					}
					if change != "unchanged" && (old.Revision >= record.Revision || old.Digest == record.Digest) {
						return fmt.Errorf("%s: changed dependency reused revision", category)
					}
				}
				user, err := CurrentUser(cacheCtx, db)
				if err != nil {
					return err
				}
				if err := cacheConfigurationRecord(cacheCtx, db, user, category, record); err != nil {
					return err
				}
				if old, ok := previous[category]; ok {
					retained, err := readConfigurationCacheRecord(cacheCtx, db, category, old.ID, old.Revision)
					if err != nil || retained == nil || retained.Digest != old.Digest {
						return fmt.Errorf("%s: captured history changed: %v", category, err)
					}
				}
				current[category] = record
			}
			previous = current
			return nil
		}); err != nil {
			t.Fatalf("%s: %v", change, err)
		}
	}
}
