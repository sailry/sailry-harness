package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// PairIdentityMember receives only the sharing endpoint's captured invitation
// scope. A repeated pairing never transfers an existing device to another user.
func PairIdentityMember(ctx context.Context, db *sql.Conn, node, peer protocol.NodeID, direction link.PeerDirection, inviter *protocol.UserID, myDevice bool) error {
	if !myDevice || !direction.AllowsInbound() {
		return PairLinkPeer(ctx, db, peer, direction)
	}
	target, err := LocalUser(ctx, db)
	if err != nil {
		return err
	}
	if inviter != nil {
		target, err = ResolveUser(ctx, db, *inviter)
		if err != nil {
			return err
		}
	}
	var previous link.PeerDirection
	var owner sql.NullString
	err = db.QueryRowContext(ctx, "SELECT direction,user FROM link_peers WHERE identity=?1", peer[:]).Scan(&previous, &owner)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	if owner.Valid && owner.String != target.ID.String() {
		return PairLinkPeer(ctx, db, peer, direction)
	}
	if previous != "" && previous != direction {
		direction = link.DirectionBoth
	}
	if err := SetLinkTrust(ctx, db, peer, true); err != nil {
		return err
	}
	if _, err := db.ExecContext(ctx, "UPDATE link_peers SET direction=?2,legacy=0,user=?3 WHERE identity=?1", peer[:], string(direction), target.ID.String()); err != nil {
		return err
	}
	return rememberIdentityMembership(ctx, db, peer, target.ID, node)
}

func rememberIdentityMembership(ctx context.Context, db *sql.Conn, device protocol.NodeID, user protocol.UserID, sponsor protocol.NodeID) error {
	_, err := db.ExecContext(ctx, `INSERT INTO identity_memberships(device,user,sponsor,generation,active) VALUES(?1,?2,?3,1,1)
ON CONFLICT(device) DO UPDATE SET generation=CASE WHEN active=1 AND user=excluded.user AND sponsor=excluded.sponsor THEN generation ELSE generation+1 END,user=excluded.user,sponsor=excluded.sponsor,active=1`, device[:], user.String(), sponsor[:])
	return err
}

func ReadIdentity(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID) (protocol.IdentityState, error) {
	user, err := CallerUser(ctx, db, node, caller)
	result := protocol.IdentityState{User: user, Users: []protocol.User{}, Devices: []protocol.UserDevice{}, Sponsorships: []protocol.DeviceSponsorship{}}
	if err != nil {
		return result, err
	}
	if user.Administrator {
		result.Users, err = ListUsers(ctx, db)
		if err != nil {
			return result, err
		}
	}
	local, err := LocalUser(ctx, db)
	if err != nil {
		return result, err
	}
	if user.Administrator || user.ID == local.ID {
		result.Devices = append(result.Devices, protocol.UserDevice{Device: node, User: local.ID, Direction: string(link.DirectionBoth), MyDevice: true})
	}
	rows, err := db.QueryContext(ctx, `SELECT p.identity,p.user,p.direction,coalesce(m.active,0)
FROM link_peers p LEFT JOIN identity_memberships m ON m.device=p.identity AND m.user=p.user
WHERE p.user IS NOT NULL AND (p.user=?1 OR ?2) ORDER BY p.identity`, user.ID.String(), user.Administrator)
	if err != nil {
		return result, err
	}
	for rows.Next() {
		var device []byte
		var id string
		var value protocol.UserDevice
		if err := rows.Scan(&device, &id, &value.Direction, &value.MyDevice); err != nil {
			rows.Close()
			return result, err
		}
		if len(device) != len(protocol.NodeID{}) {
			rows.Close()
			return result, invalid("invalid stored device identity")
		}
		value.Device = protocol.NodeID(device)
		value.User, err = protocol.ParseUserID(id)
		if err != nil {
			rows.Close()
			return result, err
		}
		result.Devices = append(result.Devices, value)
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return result, err
	}
	rows, err = db.QueryContext(ctx, `SELECT sponsor,request,device,user,state FROM identity_sponsorships
WHERE state='pending' AND (device=?1 OR user=?2 OR ?3) ORDER BY rowid`, caller[:], user.ID.String(), user.Administrator)
	if err != nil {
		return result, err
	}
	defer rows.Close()
	for rows.Next() {
		value, err := scanSponsorship(rows)
		if err != nil {
			return result, err
		}
		result.Sponsorships = append(result.Sponsorships, value)
	}
	return result, rows.Err()
}

type identityScanner interface{ Scan(...any) error }

func scanSponsorship(row identityScanner) (protocol.DeviceSponsorship, error) {
	var result protocol.DeviceSponsorship
	var sponsor, device []byte
	var request, user string
	if err := row.Scan(&sponsor, &request, &device, &user, &result.State); err != nil {
		return result, err
	}
	if len(sponsor) != len(protocol.NodeID{}) || len(device) != len(protocol.NodeID{}) {
		return result, invalid("invalid stored sponsorship")
	}
	result.Sponsor, result.Device = protocol.NodeID(sponsor), protocol.NodeID(device)
	var err error
	if result.Request, err = protocol.ParseRequestID(request); err != nil {
		return result, err
	}
	result.User, err = protocol.ParseUserID(user)
	return result, err
}

func RevokeIdentityDevice(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, input protocol.RevokeDevice) error {
	actor, err := CallerUser(ctx, db, node, caller)
	if err != nil {
		return err
	}
	if input.Device == node {
		return invalid("the built-in local device cannot be removed")
	}
	if input.Sponsored {
		return invalid("sponsored device revocation is no longer supported")
	}
	var owner sql.NullString
	err = db.QueryRowContext(ctx, "SELECT user FROM link_peers WHERE identity=?1", input.Device[:]).Scan(&owner)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	if !actor.Administrator && (owner.String != actor.ID.String() || !owner.Valid) && caller != input.Device {
		return fault(protocol.ErrorCodePermissionDenied, "device access is unavailable")
	}
	if owner.Valid {
		var last bool
		if err := db.QueryRowContext(ctx, `SELECT local=0 AND (SELECT count(*) FROM link_peers WHERE user=users.id AND direction IN ('controller','both'))=1 FROM users WHERE id=?1`, owner.String).Scan(&last); err != nil {
			return err
		}
		if last && !input.ConfirmLast {
			return invalid("confirm removal of the user's last device; its data will remain without device access")
		}
	}
	if !actor.Administrator {
		return removeIdentityInbound(ctx, db, input.Device)
	}
	return removeIdentityPeer(ctx, db, input.Device, true)
}

// Group revocation removes inbound membership. It cannot delete a host's
// independent outbound pairing to the same endpoint.
func removeIdentityInbound(ctx context.Context, db *sql.Conn, device protocol.NodeID) error {
	var outbound bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM link_peers WHERE identity=?1 AND direction IN ('host','both'))", device[:]).Scan(&outbound); err != nil {
		return err
	}
	if !outbound {
		return removeIdentityPeer(ctx, db, device, false)
	}
	if _, err := db.ExecContext(ctx, "UPDATE identity_memberships SET active=0 WHERE device=?1", device[:]); err != nil {
		return err
	}
	if _, err := db.ExecContext(ctx, "UPDATE identity_sponsorships SET state='cancelled' WHERE device=?1 OR sponsor=?1", device[:]); err != nil {
		return err
	}
	if _, err := db.ExecContext(ctx, "UPDATE link_peers SET direction='host',user=NULL WHERE identity=?1", device[:]); err != nil {
		return err
	}
	values, err := IdentityPeerRelationships(ctx, db)
	if err != nil {
		return err
	}
	for _, value := range values {
		if value.ID != device {
			continue
		}
		if value.DirectDirection.AllowsOutbound() {
			value.DirectDirection = link.DirectionHost
		} else {
			value.DirectDirection = ""
			value.MyDevice = false
		}
		value.Direct = value.DirectDirection.Valid()
		return writeIdentityPeer(ctx, db, value)
	}
	return nil
}

func removeIdentityPeer(ctx context.Context, db *sql.Conn, device protocol.NodeID, localRemoval bool) error {
	if _, err := db.ExecContext(ctx, "UPDATE identity_memberships SET active=0 WHERE device=?1", device[:]); err != nil {
		return err
	}
	if _, err := db.ExecContext(ctx, "UPDATE identity_sponsorships SET state='cancelled' WHERE device=?1 OR sponsor=?1", device[:]); err != nil {
		return err
	}
	if err := removeIdentityPeerOrigins(ctx, db, device, localRemoval); err != nil {
		return err
	}
	return SetLinkTrust(ctx, db, device, false)
}
