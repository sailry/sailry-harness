package store

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginReinstall(t *testing.T) {
	for _, explicitRevision := range []bool{false, true} {
		t.Run(fmt.Sprintf("explicit revision %v", explicitRevision), func(t *testing.T) {
			_, _, storage := schemaFixture(t)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				info := pluginTestInfo("reinstall-fixture", 'a')
				schema, err := plugins.ValidateSettings([]byte(`{"$schema":"` + plugins.SettingsSchema + `","type":"object","properties":{"label":{"type":"string"}},"additionalProperties":false}`))
				if err != nil {
					return err
				}
				info.Settings = &schema
				info, err = InstallPlugin(ctx, db, info, 0, true)
				if err != nil {
					return err
				}
				settings, err := SavePluginSettings(ctx, db, protocol.CommandSavePluginSettingsPayload{
					Package: plugins.PackageReference(info), Values: map[string]json.RawMessage{"label": json.RawMessage(`"kept"`)},
				})
				if err != nil {
					return err
				}
				current, err := ReadPlugin(ctx, db, info.Summary.Name)
				if err != nil || current == nil {
					return fmt.Errorf("read configured plugin: %v", err)
				}
				removed, err := RemovePlugin(ctx, db, info.Summary.Name, current.Summary.Revision)
				if err != nil {
					return err
				}
				if visible, err := ReadPlugin(ctx, db, info.Summary.Name); err != nil || visible != nil {
					return fmt.Errorf("removed plugin remained visible: %v", err)
				}
				if recorded, err := PluginRecorded(ctx, db, info.Summary.Name); err != nil || !recorded {
					return fmt.Errorf("removed plugin lost its tombstone: %v", err)
				}
				// A client with a captured nonzero revision still has to observe
				// the removal; zero is the installed inventory's absence token.
				if _, err := InstallPlugin(ctx, db, info, current.Summary.Revision, true); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
					return fmt.Errorf("stale pre-removal revision: %v", err)
				}
				expected := uint64(0)
				if explicitRevision {
					expected = removed.Revision
				}
				reinstalled, err := InstallPlugin(ctx, db, info, expected, true)
				if err != nil {
					return err
				}
				if reinstalled.Summary.Revision != removed.Revision+1 || reinstalled.Summary.SettingsRevision != settings.Package.SettingsRevision || !reinstalled.Summary.Enabled {
					return fmt.Errorf("reinstall changed preserved state: %+v", reinstalled.Summary)
				}
				preserved, err := ReadPluginSettings(ctx, db, plugins.PackageReference(reinstalled))
				if err != nil || string(preserved.Values["label"]) != `"kept"` {
					return fmt.Errorf("reinstall lost saved settings: %v", err)
				}
				for _, stale := range []uint64{0, current.Summary.Revision, removed.Revision} {
					if _, err := InstallPlugin(ctx, db, info, stale, true); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
						return fmt.Errorf("installed plugin accepted revision %d: %v", stale, err)
					}
				}
				disabled, err := SetPluginEnabled(ctx, db, info.Summary.Name, reinstalled.Summary.Revision, false)
				if err != nil {
					return err
				}
				updated, err := InstallPlugin(ctx, db, info, disabled.Summary.Revision, true)
				if err != nil || updated.Summary.Enabled || updated.Summary.SettingsRevision != settings.Package.SettingsRevision {
					return fmt.Errorf("installed update lost enable choice or settings: %v", err)
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestPluginConcurrentReinstall(t *testing.T) {
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	var removed protocol.PluginSummary
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info, err := InstallPlugin(ctx, db, pluginTestInfo("reinstall-fixture", 'a'), 0, true)
		if err != nil {
			return err
		}
		removed, err = RemovePlugin(ctx, db, info.Summary.Name, info.Summary.Revision)
		return err
	}); err != nil {
		t.Fatal(err)
	}

	type result struct {
		info protocol.PluginInfo
		err  error
	}
	start, results := make(chan struct{}), make(chan result, 2)
	for _, digest := range []byte{'b', 'c'} {
		go func() {
			<-start
			var value result
			value.err = storage.Node.Write(ctx, func(db *sql.Conn) error {
				var err error
				value.info, err = InstallPlugin(ctx, db, pluginTestInfo("reinstall-fixture", digest), 0, true)
				return err
			})
			results <- value
		}()
	}
	close(start)
	successes, conflicts := 0, 0
	var installed protocol.PluginInfo
	for range 2 {
		value := <-results
		if value.err == nil {
			successes++
			installed = value.info
		} else if providerFixtureFault(value.err, protocol.ErrorCodeRevisionConflict) {
			conflicts++
		} else {
			t.Errorf("concurrent reinstall: %v", value.err)
		}
	}
	if successes != 1 || conflicts != 1 || installed.Summary.Revision != removed.Revision+1 {
		t.Fatalf("reinstalls: %d successes, %d conflicts, revision %d", successes, conflicts, installed.Summary.Revision)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		current, err := ReadPlugin(ctx, db, removed.Name)
		if err != nil || current == nil || current.Summary.Digest != installed.Summary.Digest || current.Summary.Revision != installed.Summary.Revision {
			return fmt.Errorf("concurrent reinstall changed the winner: %v", err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
