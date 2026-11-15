package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"sort"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func SaveIdentityDirectory(ctx context.Context, db *sql.Conn, hosts []protocol.DirectoryHost) error {
	local, err := LocalUser(ctx, db)
	if err != nil {
		return err
	}
	sort.Slice(hosts, func(i, j int) bool { return hosts[i].Node.String() < hosts[j].Node.String() })
	body, err := json.Marshal(hosts)
	if err != nil {
		return err
	}
	var previous []byte
	var revision uint64
	err = db.QueryRowContext(ctx, "SELECT revision,hosts FROM identity_directory WHERE user=?1", local.ID.String()).Scan(&revision, &previous)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	if bytes.Equal(previous, body) {
		return nil
	}
	revision, err = nextRevision(revision)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, `INSERT INTO identity_directory(user,revision,hosts) VALUES(?1,?2,?3)
ON CONFLICT(user) DO UPDATE SET revision=excluded.revision,hosts=excluded.hosts`, local.ID.String(), revision, body)
	return err
}

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
