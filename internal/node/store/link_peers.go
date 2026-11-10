package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func LinkPeers(ctx context.Context, db *sql.Conn) ([]protocol.NodeID, error) {
	rows, err := db.QueryContext(ctx, "SELECT identity FROM link_peers ORDER BY identity")
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	values := []protocol.NodeID{}
	for rows.Next() {
		var identity []byte
		if err := rows.Scan(&identity); err != nil {
			return nil, err
		}
		if len(identity) != len(protocol.NodeID{}) {
			return nil, errors.New("invalid stored peer identity")
		}
		values = append(values, protocol.NodeID(identity))
	}
	return values, rows.Err()
}

func LinkAddresses(ctx context.Context, db *sql.Conn) ([]link.EndpointAddress, error) {
	rows, err := db.QueryContext(ctx, "SELECT address FROM link_addresses ORDER BY identity")
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	values := []link.EndpointAddress{}
	for rows.Next() {
		var body string
		if err := rows.Scan(&body); err != nil {
			return nil, err
		}
		var address link.EndpointAddress
		if err := json.Unmarshal([]byte(body), &address); err != nil {
			return nil, errors.New("invalid stored peer address")
		}
		values = append(values, address)
	}
	return values, rows.Err()
}

func RememberLinkPeer(ctx context.Context, db *sql.Conn, address link.EndpointAddress) error {
	body, err := json.Marshal(address)
	if err != nil {
		return err
	}
	if len(body) > 8192 || len(address.Addrs) > 16 {
		return invalid("peer address exceeds limits")
	}
	_, err = db.ExecContext(ctx, "INSERT INTO link_addresses(identity,address) VALUES(?1,?2) ON CONFLICT(identity) DO UPDATE SET address=excluded.address", address.ID[:], string(body))
	return err
}

func SetLinkTrust(ctx context.Context, db *sql.Conn, peer protocol.NodeID, trusted bool) error {
	if !trusted {
		_, err := db.ExecContext(ctx, "DELETE FROM link_peers WHERE identity=?1", peer[:])
		return err
	}
	var count int
	var present bool
	if err := db.QueryRowContext(ctx, "SELECT count(*), EXISTS(SELECT 1 FROM link_peers WHERE identity=?1) FROM link_peers", peer[:]).Scan(&count, &present); err != nil {
		return err
	}
	if count >= 128 && !present {
		return fault(protocol.ErrorCodeBusy, "paired device capacity exhausted")
	}
	_, err := db.ExecContext(ctx, "INSERT OR IGNORE INTO link_peers(identity) VALUES(?1)", peer[:])
	return err
}

func ReadNetworkSelection(ctx context.Context, db *sql.Conn) (uint64, *protocol.NetworkSelection, error) {
	stored, err := readBody[networkStored](ctx, db, "SELECT body FROM network_settings WHERE singleton=1")
	if err != nil || stored == nil {
		return 0, nil, err
	}
	return stored.Revision, &stored.Selection, nil
}

func SaveNetworkSelection(ctx context.Context, db *sql.Conn, expected uint64, selection protocol.NetworkSelection) (uint64, error) {
	revision, _, err := ReadNetworkSelection(ctx, db)
	if err != nil {
		return 0, err
	}
	if err := checkRevision(revision, expected); err != nil {
		return 0, err
	}
	revision, err = nextRevision(revision)
	if err != nil {
		return 0, err
	}
	body, err := encode(networkStored{Revision: revision, Selection: selection})
	if err != nil {
		return 0, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO network_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body)
	return revision, err
}
