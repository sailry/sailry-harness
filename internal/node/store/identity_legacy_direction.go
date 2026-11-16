package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func NarrowLegacyController(ctx context.Context, db *sql.Conn, peer protocol.NodeID) error {
	result, err := db.ExecContext(ctx, "UPDATE link_peers SET direction='controller',legacy=0 WHERE identity=?1 AND legacy=1 AND direction='both'", peer[:])
	if err != nil {
		return err
	}
	count, err := result.RowsAffected()
	if err != nil || count == 0 {
		return err
	}
	_, err = db.ExecContext(ctx, "UPDATE identity_peer_origins SET direct_direction='controller' WHERE peer=?1 AND direct=1", peer[:])
	return err
}
