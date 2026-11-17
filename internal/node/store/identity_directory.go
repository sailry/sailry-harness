package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadIdentityDirectory(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID) (protocol.HostDirectory, error) {
	result := protocol.HostDirectory{Source: node, Hosts: []protocol.DirectoryHost{}}
	local, err := LocalUser(ctx, db)
	if err != nil {
		return result, err
	}
	if caller != node {
		var allowed bool
		if err := db.QueryRowContext(ctx, `SELECT EXISTS(SELECT 1 FROM identity_memberships m JOIN link_peers p ON p.identity=m.device
WHERE m.device=?1 AND m.user=?2 AND p.user=m.user AND p.direction IN ('controller','both') AND m.sponsor=?3 AND m.active=1)`, caller[:], local.ID.String(), node[:]).Scan(&allowed); err != nil {
			return result, err
		}
		if !allowed {
			return result, fault(protocol.ErrorCodePermissionDenied, "host directory is available only to the source's my-devices")
		}
	}
	var body []byte
	err = db.QueryRowContext(ctx, "SELECT revision,hosts FROM identity_directory WHERE user=?1", local.ID.String()).Scan(&result.Revision, &body)
	if errors.Is(err, sql.ErrNoRows) {
		return result, nil
	}
	if err != nil {
		return result, err
	}
	err = json.Unmarshal(body, &result.Hosts)
	return result, err
}
