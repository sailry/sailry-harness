package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSharedPluginVersionExcludesInstallationPin(t *testing.T) {
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		newer, err := InstallPlugin(ctx, db, catalogFixtureInfo("2.0.0", 'b'), 0, true)
		if err != nil {
			return err
		}
		older := catalogFixtureInfo("1.0.0", 'a')
		switched, err := InstallCatalogPlugin(ctx, db, older, newer.Summary.Revision, protocol.PluginInstallModeSwitch, protocol.PluginMcpConfiguration{})
		if err != nil || !switched.Summary.Pinned {
			return fmt.Errorf("downgrade was not pinned: %+v, %v", switched.Summary, err)
		}
		shared, err := readBody[protocol.PluginInfo](ctx, db, "SELECT body FROM plugin_packages WHERE digest=?1", older.Summary.Digest)
		if err != nil || shared == nil || shared.Summary.Pinned {
			return fmt.Errorf("immutable package contains an installation preference: %+v, %v", shared, err)
		}
		shared.Summary.Pinned = true
		legacy, err := encode(*shared)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE plugin_packages SET body=?1 WHERE digest=?2", legacy, older.Summary.Digest); err != nil {
			return err
		}
		controller := protocol.NodeID{44}
		if err := PairPeerIdentity(ctx, db, node, controller, link.DirectionController, true); err != nil {
			return err
		}
		second, err := CallerUser(ctx, db, node, controller)
		if err != nil {
			return err
		}
		owner := WithUser(ctx, second)
		installed, err := InstallPlugin(owner, db, older, 0, true)
		if err != nil || installed.Summary.Pinned {
			return fmt.Errorf("second installation should follow updates: %+v, %v", installed.Summary, err)
		}
		version, err := ReadPluginVersion(owner, db, plugins.PackageReference(installed))
		if err != nil || version.Summary.Pinned || version.Summary.Digest != older.Summary.Digest {
			return fmt.Errorf("shared version returned the first installer's pin: %+v, %v", version.Summary, err)
		}
		current, err := ReadPlugin(ctx, db, older.Summary.Name)
		if err != nil || current == nil || !current.Summary.Pinned {
			return fmt.Errorf("first installation lost its pin: %+v, %v", current, err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
