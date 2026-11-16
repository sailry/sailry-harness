package store

import (
	"database/sql"
	"errors"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestKnownLegacyControllerNarrowingPreservesExplicitPairings(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		legacy, explicit, unknown := protocol.NodeID{2}, protocol.NodeID{3}, protocol.NodeID{4}
		if err := SetLinkTrust(ctx, db, legacy, true); err != nil {
			return err
		}
		for _, direction := range []link.PeerDirection{link.DirectionController, link.DirectionHost} {
			if err := PairLinkPeer(ctx, db, explicit, direction); err != nil {
				return err
			}
		}
		before, err := CallerUser(ctx, db, node, legacy)
		if err != nil {
			return err
		}
		for _, peer := range []protocol.NodeID{legacy, explicit, unknown} {
			if err := NarrowLegacyController(ctx, db, peer); err != nil {
				return err
			}
		}
		values, err := LinkDirections(ctx, db)
		if err != nil {
			return err
		}
		if len(values) != 2 || values[0].Direction != link.DirectionController || values[0].Legacy || values[1].Direction != link.DirectionBoth || values[1].Legacy {
			return errors.New("legacy narrowing changed a newer explicit relationship or paired an unknown peer")
		}
		after, err := CallerUser(ctx, db, node, legacy)
		if err != nil || before.ID != after.ID {
			return errors.New("narrowing changed original inbound membership")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
