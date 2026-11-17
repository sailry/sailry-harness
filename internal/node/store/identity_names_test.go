package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestIdentityPeerNameColumnMigrationPreservesTrust(t *testing.T) {
	t.Parallel()
	profile, node, storage := schemaFixture(t)
	ctx := t.Context()
	peer := protocol.NodeID{2}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		return PairIdentityPeer(ctx, db, node, peer, link.DirectionHost, link.InvitationScope{Kind: protocol.InvitationOtherUser})
	}); err != nil {
		t.Fatal(err)
	}
	schemaExecute(t, storage.Node, "ALTER TABLE identity_peer_origins DROP COLUMN name")
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	migrated := reopenSchemaFixture(t, profile, node)
	if err := migrated.Node.Read(ctx, func(db *sql.Conn) error {
		values, err := IdentityPeerRelationships(ctx, db)
		if err != nil {
			return err
		}
		if len(values) != 1 || values[0].ID != peer || values[0].Name != "" || values[0].DirectDirection != link.DirectionHost {
			return fmt.Errorf("legacy peer changed during name migration: %#v", values)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
