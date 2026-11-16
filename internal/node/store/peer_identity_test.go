package store

import (
	"database/sql"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPeerClassificationKeepsLocalShellAndIndependentControllers(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		local, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		phone, desktop, executor := protocol.NodeID{2}, protocol.NodeID{3}, protocol.NodeID{4}
		if err := PairPeerIdentity(ctx, db, node, phone, link.DirectionController, false); err != nil {
			return err
		}
		var users int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM users").Scan(&users); err != nil || users != 1 {
			return fmt.Errorf("shell pairing created an unused identity: users=%d, %v", users, err)
		}
		shell, err := CallerUser(ctx, db, node, phone)
		if err != nil || shell.ID != local.ID || shell.Controller != nil {
			return errors.New("phone did not use the local owner's identity")
		}
		if err := PairPeerIdentity(ctx, db, node, desktop, link.DirectionController, true); err != nil {
			return err
		}
		remote, err := CallerUser(ctx, db, node, desktop)
		if err != nil || remote.ID == local.ID || remote.Controller == nil || *remote.Controller != desktop {
			return errors.New("desktop did not retain its separate controller identity")
		}
		for range 2 {
			if err := ClassifyPeer(ctx, db, node, desktop, true); err != nil {
				return err
			}
		}
		again, err := CallerUser(ctx, db, node, desktop)
		if err != nil || again.ID != remote.ID {
			return errors.New("reconnection changed the controller's identity")
		}
		controller, err := CurrentController(WithUser(ctx, remote), db, node)
		if err != nil || controller != desktop {
			return errors.New("current controller was not derived from authenticated membership")
		}
		if err := PairPeerIdentity(ctx, db, node, executor, link.DirectionHost, true); err != nil {
			return err
		}
		if _, err := CallerUser(ctx, db, node, executor); err == nil {
			return errors.New("outbound execution host gained inbound ownership")
		}
		if err := ClassifyPeer(ctx, db, node, protocol.NodeID{9}, true); err == nil {
			return errors.New("classification paired an unknown device")
		}
		if err := ClassifyPeer(ctx, db, node, desktop, false); err == nil {
			return errors.New("reconnection changed a classified device type")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestLegacyPeerClassificationPreservesDataAndSplitsSharedMembership(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		local, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		desktop, phone := protocol.NodeID{2}, protocol.NodeID{3}
		for _, peer := range []protocol.NodeID{desktop, phone} {
			if err := SetLinkTrust(ctx, db, peer, true); err != nil {
				return err
			}
		}
		project := protocol.NewProjectID()
		if _, err := db.ExecContext(ctx, "INSERT INTO projects(id,name,path,appearance,owner) VALUES(?1,'Legacy project','/legacy-classification','{}',?2)", project.String(), local.ID.String()); err != nil {
			return err
		}
		origin, err := LegacyStorageUser(ctx, db)
		if err != nil {
			return err
		}
		if err := ClassifyPeer(ctx, db, node, desktop, true); err != nil {
			return err
		}
		if err := ClassifyPeer(ctx, db, node, phone, false); err != nil {
			return err
		}
		remote, err := CallerUser(ctx, db, node, desktop)
		if err != nil || remote.ID == local.ID {
			return errors.New("legacy desktop retained the local owner's private scope")
		}
		var owner string
		if err := db.QueryRowContext(ctx, "SELECT owner FROM projects WHERE id=?1", project.String()).Scan(&owner); err != nil || owner != local.ID.String() {
			return errors.New("classification reassigned a legacy project")
		}
		after, err := LegacyStorageUser(ctx, db)
		if err != nil || after != origin {
			return errors.New("classification changed immutable storage ownership")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestExistingIndependentUserIsReusedByClassification(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		peer := protocol.NodeID{2}
		if err := PairIdentityPeer(ctx, db, node, peer, link.DirectionController, link.InvitationScope{Kind: protocol.InvitationOtherUser}); err != nil {
			return err
		}
		before, err := CallerUser(ctx, db, node, peer)
		if err != nil {
			return err
		}
		if err := ClassifyPeer(ctx, db, node, peer, true); err != nil {
			return err
		}
		after, err := CallerUser(ctx, db, node, peer)
		if err != nil || before.ID != after.ID || after.Controller == nil || *after.Controller != peer {
			return errors.New("classification discarded an existing independent user")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
