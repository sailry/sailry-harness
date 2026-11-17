package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"

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
		if err := PairLinkPeer(ctx, db, peer, direction); err != nil {
			return err
		}
		return proposeIdentityPairing(ctx, db, node, peer, target)
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

// A known device's my-device invitation is an explicit proposal, never an
// implicit reassignment. The claimant persists the authenticated acknowledgment
// and then confirms this stored proposal under its own controller identity.
func proposeIdentityPairing(ctx context.Context, db *sql.Conn, node, device protocol.NodeID, target protocol.User) error {
	var present bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM identity_sponsorships WHERE sponsor=?1 AND device=?2 AND user=?3 AND state='pending')", node[:], device[:], target.ID.String()).Scan(&present); err != nil {
		return err
	}
	if present {
		return nil
	}
	if _, err := db.ExecContext(ctx, "UPDATE identity_sponsorships SET state='cancelled' WHERE sponsor=?1 AND device=?2 AND state='pending'", node[:], device[:]); err != nil {
		return err
	}
	address, err := json.Marshal(link.EndpointAddress{ID: device, Addrs: []json.RawMessage{}})
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO identity_sponsorships(sponsor,request,device,user,address,state) VALUES(?1,?2,?3,?4,?5,'pending')", node[:], protocol.NewRequestID().String(), device[:], target.ID.String(), string(address))
	return err
}

func rememberIdentityMembership(ctx context.Context, db *sql.Conn, device protocol.NodeID, user protocol.UserID, sponsor protocol.NodeID) error {
	_, err := db.ExecContext(ctx, `INSERT INTO identity_memberships(device,user,sponsor,generation,active) VALUES(?1,?2,?3,1,1)
ON CONFLICT(device) DO UPDATE SET generation=CASE WHEN active=1 AND user=excluded.user AND sponsor=excluded.sponsor THEN generation ELSE generation+1 END,user=excluded.user,sponsor=excluded.sponsor,active=1`, device[:], user.String(), sponsor[:])
	return err
}

func identityAddress(device protocol.NodeID, body string) (link.EndpointAddress, error) {
	var address link.EndpointAddress
	if len(body) > 8192 || json.Unmarshal([]byte(body), &address) != nil || address.ID != device || len(address.Addrs) > 16 {
		return address, invalid("invalid device address")
	}
	return address, nil
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

// SponsorIdentityDevice and ConfirmIdentitySponsorship are retired from the
// identity command, which rejects both. They stay for the same reason as
// MergeIdentityUsers: tests build the sponsored memberships that earlier
// versions stored, which revocation still has to handle.
func SponsorIdentityDevice(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.RequestID, input protocol.SponsorDevice) (protocol.DeviceSponsorship, error) {
	result := protocol.DeviceSponsorship{Sponsor: caller, Request: request, Device: input.Device}
	actor, err := CallerUser(ctx, db, node, caller)
	if err != nil {
		return result, err
	}
	result.User = actor.ID
	if input.Device == node || input.Device == caller || input.Device == (protocol.NodeID{}) {
		return result, invalid("invalid sponsored device")
	}
	if input.Generation == 0 || input.Generation > math.MaxInt64 {
		return result, invalid("invalid sponsorship generation")
	}
	address, err := identityAddress(input.Device, input.Address)
	if err != nil {
		return result, err
	}
	existing, err := scanSponsorship(db.QueryRowContext(ctx, "SELECT sponsor,request,device,user,state FROM identity_sponsorships WHERE sponsor=?1 AND request=?2", caller[:], request.String()))
	if err == nil {
		if existing.Device != input.Device || existing.User != actor.ID {
			return result, fault(protocol.ErrorCodeRevisionConflict, "sponsorship identity was reused")
		}
		return existing, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return result, err
	}
	var removed bool
	if err := db.QueryRowContext(ctx, "SELECT coalesce((SELECT removed FROM identity_peer_origins WHERE peer=?1),0)", input.Device[:]).Scan(&removed); err != nil {
		return result, err
	}
	var generation uint64
	var revoked bool
	err = db.QueryRowContext(ctx, "SELECT generation,revoked FROM identity_sponsor_epochs WHERE sponsor=?1 AND device=?2", caller[:], input.Device[:]).Scan(&generation, &revoked)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return result, err
	}
	if removed || input.Generation < generation || input.Generation == generation && revoked {
		result.State = "cancelled"
		_, err = db.ExecContext(ctx, "INSERT INTO identity_sponsorships(sponsor,request,device,user,address,state) VALUES(?1,?2,?3,?4,?5,'cancelled')", caller[:], request.String(), input.Device[:], actor.ID.String(), input.Address)
		return result, err
	}
	if _, err = db.ExecContext(ctx, `INSERT INTO identity_sponsor_epochs(sponsor,device,generation,revoked) VALUES(?1,?2,?3,0) ON CONFLICT(sponsor,device) DO UPDATE SET generation=excluded.generation,revoked=0`, caller[:], input.Device[:], input.Generation); err != nil {
		return result, err
	}
	var owner sql.NullString
	var previous link.PeerDirection
	err = db.QueryRowContext(ctx, "SELECT user,direction FROM link_peers WHERE identity=?1", input.Device[:]).Scan(&owner, &previous)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return result, err
	}
	result.State = "pending"
	if !owner.Valid || owner.String == actor.ID.String() {
		alreadyMember := owner.Valid && previous.AllowsInbound()
		if err := SetLinkTrust(ctx, db, input.Device, true); err != nil {
			return result, err
		}
		direction := link.DirectionController
		if previous.AllowsOutbound() {
			direction = link.DirectionBoth
		}
		if _, err := db.ExecContext(ctx, "UPDATE link_peers SET direction=?2,legacy=0,user=?3 WHERE identity=?1", input.Device[:], string(direction), actor.ID.String()); err != nil {
			return result, err
		}
		if err := RememberLinkPeer(ctx, db, address); err != nil {
			return result, err
		}
		if !alreadyMember {
			if err := rememberIdentityMembership(ctx, db, input.Device, actor.ID, caller); err != nil {
				return result, err
			}
		}
		result.State = "accepted"
	}
	_, err = db.ExecContext(ctx, "INSERT INTO identity_sponsorships(sponsor,request,device,user,address,state) VALUES(?1,?2,?3,?4,?5,?6)", caller[:], request.String(), input.Device[:], actor.ID.String(), input.Address, result.State)
	return result, err
}

func ConfirmIdentitySponsorship(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, input protocol.ConfirmSponsorship) (protocol.DeviceSponsorship, error) {
	result, err := scanSponsorship(db.QueryRowContext(ctx, "SELECT sponsor,request,device,user,state FROM identity_sponsorships WHERE sponsor=?1 AND request=?2", input.Sponsor[:], input.Request.String()))
	if errors.Is(err, sql.ErrNoRows) || err == nil && result.Device != caller {
		return result, fault(protocol.ErrorCodeNotFound, "sponsorship is unavailable")
	}
	if err != nil {
		return result, err
	}
	if result.State == "cancelled" {
		return result, fault(protocol.ErrorCodePermissionDenied, "sponsorship was cancelled")
	}
	sponsor, err := CallerUser(ctx, db, node, input.Sponsor)
	if input.Sponsor == node {
		sponsor, err = ResolveUser(ctx, db, result.User)
	}
	if err != nil || sponsor.ID != result.User {
		return result, fault(protocol.ErrorCodePermissionDenied, "sponsorship is unavailable")
	}
	current, err := CallerUser(ctx, db, node, caller)
	if err != nil {
		return result, err
	}
	if result.State == "accepted" {
		if current.ID != sponsor.ID {
			return result, fault(protocol.ErrorCodeRevisionConflict, "device membership changed")
		}
		return result, nil
	}
	if current.ID != sponsor.ID {
		if _, err := MergeIdentityUsers(ctx, db, current.ID, sponsor.ID); err != nil {
			return result, err
		}
	}
	if err := rememberIdentityMembership(ctx, db, caller, sponsor.ID, input.Sponsor); err != nil {
		return result, err
	}
	_, err = db.ExecContext(ctx, "UPDATE identity_sponsorships SET state='accepted' WHERE sponsor=?1 AND request=?2", input.Sponsor[:], input.Request.String())
	result.State = "accepted"
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
		if input.Generation == 0 || input.Generation > math.MaxInt64 {
			return invalid("invalid sponsorship generation")
		}
		var previous uint64
		err := db.QueryRowContext(ctx, "SELECT generation FROM identity_sponsor_epochs WHERE sponsor=?1 AND device=?2", caller[:], input.Device[:]).Scan(&previous)
		if err != nil && !errors.Is(err, sql.ErrNoRows) {
			return err
		}
		if input.Generation < previous {
			return nil
		}
		if _, err := db.ExecContext(ctx, `INSERT INTO identity_sponsor_epochs(sponsor,device,generation,revoked) VALUES(?1,?2,?3,1) ON CONFLICT(sponsor,device) DO UPDATE SET generation=excluded.generation,revoked=1`, caller[:], input.Device[:], input.Generation); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE identity_sponsorships SET state='cancelled' WHERE sponsor=?1 AND device=?2", caller[:], input.Device[:]); err != nil {
			return err
		}
	}
	var owner sql.NullString
	err = db.QueryRowContext(ctx, "SELECT user FROM link_peers WHERE identity=?1", input.Device[:]).Scan(&owner)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	if input.Sponsored {
		var sponsored bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM identity_memberships WHERE sponsor=?1 AND device=?2 AND user=?3 AND active=1)", caller[:], input.Device[:], actor.ID.String()).Scan(&sponsored); err != nil {
			return err
		}
		if !sponsored || !owner.Valid || owner.String != actor.ID.String() {
			return nil
		}
	} else if !actor.Administrator && (owner.String != actor.ID.String() || !owner.Valid) && caller != input.Device {
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
	if input.Sponsored || !actor.Administrator {
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
