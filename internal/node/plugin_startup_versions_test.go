package node

import (
	"database/sql"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPinnedBundledVersionSurvivesInitialization(t *testing.T) {
	node := nodeFixture(t)
	ctx := t.Context()
	const name = "memory"
	const removedName = "progress"
	source := t.TempDir()
	manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"` + name + `","version":"0.0.0"}`
	if err := os.WriteFile(filepath.Join(source, "plugin.json"), []byte(manifest), 0o600); err != nil {
		t.Fatal(err)
	}
	older, err := node.plugins.manager.Install(ctx, source, "", name)
	if err != nil {
		t.Fatal(err)
	}
	older.Origin = &protocol.PluginOrigin{Kind: "bundled"}
	older.UpdateSource = &protocol.PluginOrigin{Kind: "official"}
	var user protocol.User
	var bundled, pinned protocol.PluginInfo
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		user, err = store.LocalUser(ctx, db)
		if err != nil {
			return err
		}
		current, err := store.ReadPlugin(ctx, db, name)
		if err != nil || current == nil {
			return fmt.Errorf("bundled package: %+v %v", current, err)
		}
		bundled = *current
		if !plugins.IsNewer(older.Summary.Version, bundled.Summary.Version) {
			return fmt.Errorf("fixture is not older than the bundled version: %+v", bundled.Summary)
		}
		disabled, err := store.SetPluginEnabled(ctx, db, name, current.Summary.Revision, false)
		if err != nil {
			return err
		}
		older, err = store.InstallPlugin(ctx, db, older, disabled.Summary.Revision, false)
		if err != nil {
			return err
		}
		pinned, err = store.SetPluginPinned(ctx, db, name, older.Summary.Revision, true)
		if err != nil {
			return err
		}
		removed, err := store.ReadPlugin(ctx, db, removedName)
		if err != nil || removed == nil {
			return fmt.Errorf("removal fixture: %+v %v", removed, err)
		}
		_, err = store.RemovePlugin(ctx, db, removedName, removed.Summary.Revision)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := node.InitializeUserPlugins(ctx, user); err != nil {
		t.Fatal(err)
	}
	var following protocol.PluginInfo
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		current, err := store.ReadPlugin(ctx, db, name)
		if err != nil || current == nil || !reflect.DeepEqual(current.Summary, pinned.Summary) {
			return fmt.Errorf("initialization changed the pinned package: %+v %v", current, err)
		}
		removed, err := store.ReadPlugin(ctx, db, removedName)
		if err != nil || removed != nil {
			return fmt.Errorf("initialization restored a removed package: %+v %v", removed, err)
		}
		following, err = store.SetPluginPinned(ctx, db, name, current.Summary.Revision, false)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := node.InitializeUserPlugins(ctx, user); err != nil {
		t.Fatal(err)
	}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		current, err := store.ReadPlugin(ctx, db, name)
		if err != nil || current == nil || current.Summary.Digest != bundled.Summary.Digest || current.Summary.Pinned || current.Summary.Enabled || current.Summary.Revision != following.Summary.Revision+1 {
			return fmt.Errorf("following did not resume the bundled upgrade: %+v %v", current, err)
		}
		removed, err := store.ReadPlugin(ctx, db, removedName)
		if err != nil || removed != nil {
			return fmt.Errorf("following restored a removed package: %+v %v", removed, err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
