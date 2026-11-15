package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestIdentityDirectoryRetainsHostNames(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		source, direct, imported := protocol.NodeID{2}, protocol.NodeID{3}, protocol.NodeID{4}
		if err := PairIdentityPeer(ctx, db, node, source, link.DirectionHost, link.InvitationScope{Kind: protocol.InvitationMyDevice}); err != nil {
			return err
		}
		if err := PairIdentityPeer(ctx, db, node, direct, link.DirectionHost, link.InvitationScope{Kind: protocol.InvitationOtherUser}); err != nil {
			return err
		}
		if err := RememberIdentityPeerName(ctx, db, direct, "Direct host"); err != nil {
			return err
		}
		addresses := []link.EndpointAddress{{ID: direct}, {ID: imported}}
		if err := ApplyIdentityDirectory(ctx, db, source, 1, addresses, map[protocol.NodeID]string{direct: "Directory alias", imported: "Imported host"}); err != nil {
			return err
		}
		if err := ApplyIdentityDirectory(ctx, db, source, 2, addresses, map[protocol.NodeID]string{imported: "Renamed imported host"}); err != nil {
			return err
		}
		if err := ApplyIdentityDirectory(ctx, db, source, 1, addresses, map[protocol.NodeID]string{imported: "Stale name"}); err != nil {
			return err
		}
		values, err := IdentityPeerRelationships(ctx, db)
		if err != nil {
			return err
		}
		names := map[protocol.NodeID]string{}
		for _, value := range values {
			names[value.ID] = value.Name
		}
		if names[direct] != "Direct host" || names[imported] != "Renamed imported host" {
			return fmt.Errorf("remembered host names: %#v", names)
		}
		if err := SaveIdentityDirectory(ctx, db, []protocol.DirectoryHost{{Node: direct, Name: names[direct], Address: identityTestAddress(direct)}}); err != nil {
			return err
		}
		directory, err := ReadIdentityDirectory(ctx, db, node, node)
		if err != nil {
			return err
		}
		if len(directory.Hosts) != 1 || directory.Hosts[0].Name != names[direct] {
			return fmt.Errorf("exported host directory lost its remembered name: %#v", directory)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

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
