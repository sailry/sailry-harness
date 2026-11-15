package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"slices"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func IdentityPeerRelationships(ctx context.Context, db *sql.Conn) ([]link.PeerRelationship, error) {
	rows, err := db.QueryContext(ctx, `SELECT p.identity,coalesce(o.direct,1),coalesce(o.my_device,0),coalesce(o.removed,0),coalesce(o.sources,'[]'),coalesce(o.directory_revision,0),coalesce(nullif(o.direct_direction,''),CASE WHEN coalesce(o.direct,1)=1 THEN p.direction ELSE '' END) FROM link_peers p LEFT JOIN identity_peer_origins o ON o.peer=p.identity
UNION ALL SELECT peer,direct,my_device,removed,sources,directory_revision,direct_direction FROM identity_peer_origins WHERE NOT EXISTS(SELECT 1 FROM link_peers WHERE identity=peer) ORDER BY 1`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	values := []link.PeerRelationship{}
	for rows.Next() {
		var value link.PeerRelationship
		var id, sources []byte
		if err := rows.Scan(&id, &value.Direct, &value.MyDevice, &value.Removed, &sources, &value.DirectoryRevision, &value.DirectDirection); err != nil {
			return nil, err
		}
		if len(id) != len(protocol.NodeID{}) || json.Unmarshal(sources, &value.Sources) != nil {
			return nil, invalid("invalid stored peer provenance")
		}
		value.ID = protocol.NodeID(id)
		values = append(values, value)
	}
	return values, rows.Err()
}

func writeIdentityPeer(ctx context.Context, db *sql.Conn, value link.PeerRelationship) error {
	body, err := json.Marshal(value.Sources)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, `INSERT INTO identity_peer_origins(peer,direct,my_device,removed,sources,directory_revision,direct_direction) VALUES(?1,?2,?3,?4,?5,?6,?7)
ON CONFLICT(peer) DO UPDATE SET direct=excluded.direct,my_device=excluded.my_device,removed=excluded.removed,sources=excluded.sources,directory_revision=excluded.directory_revision,direct_direction=excluded.direct_direction`, value.ID[:], value.Direct, value.MyDevice, value.Removed, body, value.DirectoryRevision, string(value.DirectDirection))
	return err
}

func PairIdentityPeer(ctx context.Context, db *sql.Conn, node, peer protocol.NodeID, direction link.PeerDirection, scope link.InvitationScope) error {
	if err := PairIdentityMember(ctx, db, node, peer, direction, scope.User, scope.Kind == protocol.InvitationMyDevice); err != nil {
		return err
	}
	values, err := IdentityPeerRelationships(ctx, db)
	if err != nil {
		return err
	}
	value := link.PeerRelationship{ID: peer, Sources: []protocol.NodeID{}}
	for _, previous := range values {
		if previous.ID == peer {
			value = previous
			break
		}
	}
	if value.Removed {
		value.DirectoryRevision = 0
		value.DirectDirection = ""
	}
	value.Direct, value.Removed = true, false
	value.DirectDirection = mergeIdentityDirection(value.DirectDirection, direction)
	value.MyDevice = value.MyDevice || scope.Kind == protocol.InvitationMyDevice
	return writeIdentityPeer(ctx, db, value)
}

func removeIdentityPeerOrigins(ctx context.Context, db *sql.Conn, peer protocol.NodeID, localRemoval bool) error {
	values, err := IdentityPeerRelationships(ctx, db)
	if err != nil {
		return err
	}
	for _, value := range values {
		if value.ID == peer {
			value.Direct, value.MyDevice, value.Removed = false, false, localRemoval
			value.DirectDirection = ""
		}
		value.Sources = slices.DeleteFunc(value.Sources, func(source protocol.NodeID) bool { return source == peer })
		if err := writeIdentityPeer(ctx, db, value); err != nil {
			return err
		}
		if !value.DirectDirection.AllowsOutbound() && len(value.Sources) == 0 && value.ID != peer && !value.Removed {
			if err := pruneIdentityOutbound(ctx, db, value.ID); err != nil {
				return err
			}
		}
	}
	return nil
}

func ApplyIdentityDirectory(ctx context.Context, db *sql.Conn, source protocol.NodeID, revision uint64, addresses []link.EndpointAddress) error {
	values, err := IdentityPeerRelationships(ctx, db)
	if err != nil {
		return err
	}
	verified := false
	byID := map[protocol.NodeID]link.PeerRelationship{}
	for _, value := range values {
		if value.ID == source && value.MyDevice && !value.Removed {
			if revision < value.DirectoryRevision {
				return nil
			}
			verified = true
			value.DirectoryRevision = revision
		}
		value.Sources = slices.DeleteFunc(value.Sources, func(id protocol.NodeID) bool { return id == source })
		byID[value.ID] = value
	}
	if !verified {
		return fault(protocol.ErrorCodePermissionDenied, "directory source is not a paired my-device")
	}
	for _, address := range addresses {
		value, exists := byID[address.ID]
		if !exists {
			value = link.PeerRelationship{ID: address.ID, Sources: []protocol.NodeID{}}
		}
		if value.Removed {
			continue
		}
		value.Sources = append(value.Sources, source)
		byID[address.ID] = value
		if err := PairLinkPeer(ctx, db, address.ID, link.DirectionHost); err != nil {
			return err
		}
		var knownAddress bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM link_addresses WHERE identity=?1)", address.ID[:]).Scan(&knownAddress); err != nil {
			return err
		}
		if !value.Direct || !knownAddress {
			if err := RememberLinkPeer(ctx, db, address); err != nil {
				return err
			}
		}
	}
	for _, value := range byID {
		if err := writeIdentityPeer(ctx, db, value); err != nil {
			return err
		}
		if !value.DirectDirection.AllowsOutbound() && !value.Removed && len(value.Sources) == 0 {
			if err := pruneIdentityOutbound(ctx, db, value.ID); err != nil {
				return err
			}
		}
	}
	return nil
}

func mergeIdentityDirection(current, added link.PeerDirection) link.PeerDirection {
	if current == "" || current == added {
		return added
	}
	return link.DirectionBoth
}

// A directory owns outbound access only. An independently admitted controller
// remains a member when the last imported host origin disappears.
func pruneIdentityOutbound(ctx context.Context, db *sql.Conn, peer protocol.NodeID) error {
	var inbound bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM link_peers WHERE identity=?1 AND direction IN ('controller','both'))", peer[:]).Scan(&inbound); err != nil {
		return err
	}
	if inbound {
		_, err := db.ExecContext(ctx, "UPDATE link_peers SET direction='controller' WHERE identity=?1", peer[:])
		return err
	}
	if err := SetLinkTrust(ctx, db, peer, false); err != nil {
		return err
	}
	_, err := db.ExecContext(ctx, "DELETE FROM identity_peer_origins WHERE peer=?1 AND removed=0", peer[:])
	return err
}

func SetIdentityPeerDirection(ctx context.Context, db *sql.Conn, peer protocol.NodeID, direction link.PeerDirection) error {
	if err := SetLinkDirection(ctx, db, peer, direction); err != nil {
		return err
	}
	values, err := IdentityPeerRelationships(ctx, db)
	if err != nil {
		return err
	}
	for _, value := range values {
		if value.ID == peer {
			value.Direct, value.DirectDirection = true, direction
			return writeIdentityPeer(ctx, db, value)
		}
	}
	return invalid("peer is unavailable")
}
