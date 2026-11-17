package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func followFixture(t *testing.T) (*Store, protocol.NodeID, protocol.NodeID, context.Context) {
	t.Helper()
	_, node, storage := schemaFixture(t)
	source := protocol.NodeID{7}
	var user protocol.User
	if err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
		if err := PairLinkPeer(t.Context(), db, source, link.DirectionController); err != nil {
			return err
		}
		var err error
		user, err = CallerUser(t.Context(), db, node, source)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(t.Context(), "UPDATE users SET controller=?2 WHERE id=?1", user.ID.String(), source[:])
		user.Controller = &source
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return storage, node, source, WithUser(t.Context(), user)
}
func followRecord(t *testing.T, id string, revision uint64, body any, secret *protocol.Secret) protocol.ConfigurationFollowRecord {
	t.Helper()
	raw, err := json.Marshal(body)
	if err != nil {
		t.Fatal(err)
	}
	value := protocol.ConfigurationFollowRecord{ID: id, Revision: revision, Body: raw, Secret: secret}
	value.Digest, err = configurationDigest(value)
	if err != nil {
		t.Fatal(err)
	}
	return value
}
func applyFollowFixture(t *testing.T, storage *Store, ctx context.Context, node, source protocol.NodeID, category string, revision uint64, records ...protocol.ConfigurationFollowRecord) error {
	t.Helper()
	return storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := ApplyConfigurationManifest(ctx, db, node, source, protocol.ConfigurationManifest{Source: source, Category: category, Revision: revision, Records: records})
		return err
	})
}
