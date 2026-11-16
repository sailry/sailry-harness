package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// PairPeerIdentity records fresh membership and its authenticated device type in
// one transaction. A shell joins the local user without creating an orphan user.
func PairPeerIdentity(ctx context.Context, db *sql.Conn, node, peer protocol.NodeID, direction link.PeerDirection, hasNode bool) error {
	if !hasNode && direction.AllowsInbound() {
		var exists bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM link_peers WHERE identity=?1)", peer[:]).Scan(&exists); err != nil {
			return err
		}
		if !exists {
			if err := SetLinkTrust(ctx, db, peer, true); err != nil {
				return err
			}
			if err := SetLinkDirection(ctx, db, peer, direction); err != nil {
				return err
			}
		}
	}
	if err := PairIdentityPeer(ctx, db, node, peer, direction, link.InvitationScope{Kind: protocol.InvitationOtherUser}); err != nil {
		return err
	}
	return ClassifyPeer(ctx, db, node, peer, hasNode)
}

// ClassifyPeer applies the authenticated device type to an existing pairing.
// It never adds trust, changes direction, or rewrites captured history owners.
func ClassifyPeer(ctx context.Context, db *sql.Conn, node, peer protocol.NodeID, hasNode bool) error {
	var direction link.PeerDirection
	var classified sql.NullBool
	var member sql.NullString
	err := db.QueryRowContext(ctx, "SELECT direction,has_node,user FROM link_peers WHERE identity=?1", peer[:]).Scan(&direction, &classified, &member)
	if errors.Is(err, sql.ErrNoRows) {
		return fault(protocol.ErrorCodePermissionDenied, "peer is not paired")
	}
	if err != nil {
		return err
	}
	if classified.Valid && classified.Bool != hasNode {
		return fault(protocol.ErrorCodeConflict, "paired device type changed")
	}
	if !direction.AllowsInbound() {
		_, err := db.ExecContext(ctx, "UPDATE link_peers SET has_node=?2 WHERE identity=?1", peer[:], hasNode)
		return err
	}
	local, err := LocalUser(ctx, db)
	if err != nil {
		return err
	}
	owner := local.ID
	if hasNode {
		var id string
		err := db.QueryRowContext(ctx, "SELECT id FROM users WHERE controller=?1 AND local=0 AND state='active' ORDER BY id LIMIT 1", peer[:]).Scan(&id)
		switch {
		case err == nil:
			owner, err = protocol.ParseUserID(id)
			if err != nil {
				return err
			}
		case !errors.Is(err, sql.ErrNoRows):
			return err
		case member.Valid && member.String != local.ID.String():
			var count int
			var controller []byte
			if err := db.QueryRowContext(ctx, "SELECT count(*) FROM link_peers WHERE user=?1 AND direction IN ('controller','both')", member.String).Scan(&count); err != nil {
				return err
			}
			active := true
			if err := db.QueryRowContext(ctx, "SELECT controller FROM users WHERE id=?1 AND state='active'", member.String).Scan(&controller); errors.Is(err, sql.ErrNoRows) {
				active = false
			} else if err != nil {
				return err
			}
			if active && count == 1 && len(controller) == 0 {
				owner, err = protocol.ParseUserID(member.String)
			} else {
				var created protocol.User
				created, err = CreateUser(ctx, db, "Paired user")
				owner = created.ID
			}
			if err != nil {
				return err
			}
		default:
			created, err := CreateUser(ctx, db, "Paired user")
			if err != nil {
				return err
			}
			owner = created.ID
		}
		if _, err := db.ExecContext(ctx, "UPDATE users SET controller=?2 WHERE id=?1", owner.String(), peer[:]); err != nil {
			return err
		}
	}
	_, err = db.ExecContext(ctx, "UPDATE link_peers SET has_node=?2,user=?3 WHERE identity=?1", peer[:], hasNode, owner.String())
	return err
}
