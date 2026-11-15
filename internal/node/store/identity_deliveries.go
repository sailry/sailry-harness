package store

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type IdentityDelivery struct {
	Address link.EndpointAddress
	Request protocol.Request
	State   string
}

func queueIdentityDelivery(ctx context.Context, db *sql.Conn, user protocol.UserID, host link.EndpointAddress, device protocol.NodeID, generation uint64, kind string, command protocol.IdentityCommand) error {
	request := protocol.NewRequest(host.ID, protocol.Command{Kind: "identity", Data: command})
	body, err := json.Marshal(request)
	if err != nil {
		return err
	}
	address, err := json.Marshal(host)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, `INSERT INTO identity_deliveries(request,user,host,address,device,generation,kind,body,state)
VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'pending') ON CONFLICT(host,device,generation,kind) DO NOTHING`, request.ID.String(), user.String(), host.ID[:], string(address), device[:], generation, kind, body)
	return err
}

// QueueIdentitySponsors reconciles only direct my-devices sponsored by this
// Node's local user. Imported peers and other inbound tenants never fan out.
func QueueIdentitySponsors(ctx context.Context, db *sql.Conn, node protocol.NodeID, hosts []link.EndpointAddress) error {
	local, err := LocalUser(ctx, db)
	if err != nil {
		return err
	}
	rows, err := db.QueryContext(ctx, `SELECT m.device,m.generation,a.address FROM identity_memberships m JOIN link_peers p ON p.identity=m.device JOIN link_addresses a ON a.identity=m.device
WHERE m.user=?1 AND p.user=m.user AND p.direction IN ('controller','both') AND m.sponsor=?2 AND m.active=1`, local.ID.String(), node[:])
	if err != nil {
		return err
	}
	type member struct {
		device     protocol.NodeID
		generation uint64
		address    string
	}
	members := []member{}
	for rows.Next() {
		var value member
		var id []byte
		if err := rows.Scan(&id, &value.generation, &value.address); err != nil {
			rows.Close()
			return err
		}
		if len(id) != len(protocol.NodeID{}) {
			rows.Close()
			return invalid("invalid sponsored device identity")
		}
		value.device = protocol.NodeID(id)
		members = append(members, value)
	}
	if err := rows.Err(); err != nil {
		rows.Close()
		return err
	}
	if err := rows.Close(); err != nil {
		return err
	}
	for _, value := range members {
		for _, host := range hosts {
			if host.ID == value.device {
				continue
			}
			command := protocol.IdentityCommand{Kind: "sponsor_device", Data: protocol.SponsorDevice{Device: value.device, Address: value.address, Generation: value.generation}}
			if err := queueIdentityDelivery(ctx, db, local.ID, host, value.device, value.generation, "sponsor", command); err != nil {
				return err
			}
		}
	}
	return nil
}

func queueIdentityRevocations(ctx context.Context, db *sql.Conn, device protocol.NodeID) error {
	rows, err := db.QueryContext(ctx, "SELECT user,address,generation FROM identity_deliveries WHERE device=?1 AND kind='sponsor'", device[:])
	if err != nil {
		return err
	}
	type target struct {
		user       protocol.UserID
		address    link.EndpointAddress
		generation uint64
	}
	targets := []target{}
	for rows.Next() {
		var value target
		var user, address string
		if err := rows.Scan(&user, &address, &value.generation); err != nil {
			rows.Close()
			return err
		}
		if value.user, err = protocol.ParseUserID(user); err != nil {
			rows.Close()
			return err
		}
		if json.Unmarshal([]byte(address), &value.address) != nil {
			rows.Close()
			return invalid("invalid delivery host address")
		}
		targets = append(targets, value)
	}
	if err := rows.Err(); err != nil {
		rows.Close()
		return err
	}
	if err := rows.Close(); err != nil {
		return err
	}
	if _, err := db.ExecContext(ctx, "UPDATE identity_deliveries SET state='cancelled' WHERE device=?1 AND kind='sponsor' AND state IN ('pending','unknown')", device[:]); err != nil {
		return err
	}
	for _, target := range targets {
		command := protocol.IdentityCommand{Kind: "revoke_device", Data: protocol.RevokeDevice{Device: device, ConfirmLast: true, Sponsored: true, Generation: target.generation}}
		if err := queueIdentityDelivery(ctx, db, target.user, target.address, device, target.generation, "revoke", command); err != nil {
			return err
		}
	}
	return nil
}

func IdentityDeliveries(ctx context.Context, db *sql.Conn) ([]IdentityDelivery, error) {
	rows, err := db.QueryContext(ctx, "SELECT address,body,state FROM identity_deliveries WHERE state IN ('pending','unknown') ORDER BY rowid")
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	values := []IdentityDelivery{}
	for rows.Next() {
		var value IdentityDelivery
		var address string
		var body []byte
		if err := rows.Scan(&address, &body, &value.State); err != nil {
			return nil, err
		}
		if json.Unmarshal([]byte(address), &value.Address) != nil || json.Unmarshal(body, &value.Request) != nil || value.Request.Target != value.Address.ID {
			return nil, invalid("invalid stored identity delivery")
		}
		values = append(values, value)
	}
	return values, rows.Err()
}

// MarkIdentityDelivery changes only a live queue entry. A completed network
// operation racing with local revocation cannot resurrect its cancelled grant.
func MarkIdentityDelivery(ctx context.Context, db *sql.Conn, request protocol.RequestID, state string, failure *protocol.Fault) (bool, error) {
	if state != "pending" && state != "unknown" && state != "completed" && state != "blocked" {
		return false, invalid("invalid delivery state")
	}
	var body []byte
	var err error
	if failure != nil {
		body, err = json.Marshal(failure)
		if err != nil {
			return false, err
		}
	}
	result, err := db.ExecContext(ctx, "UPDATE identity_deliveries SET state=?2,fault=?3 WHERE request=?1 AND state IN ('pending','unknown')", request.String(), state, body)
	if err != nil {
		return false, err
	}
	count, err := result.RowsAffected()
	return count != 0, err
}
