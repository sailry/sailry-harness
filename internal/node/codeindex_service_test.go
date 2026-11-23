package node

import (
	"database/sql"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCodeIndexUnavailablePreservesSettings(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	ctx := t.Context()
	var saved protocol.CodeIndexSettings
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		saved, err = store.SaveCodeIndexSettings(ctx, db, protocol.CodeIndexSettings{Enabled: true, Persistent: new(true)}, 0)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	changed := saved
	changed.Enabled, changed.Persistent = false, new(false)
	command := protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{Settings: changed, ExpectedRevision: saved.Revision}}
	result := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), command))
	if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeUnavailable {
		t.Fatalf("save with no code index = %+v", result)
	}
	var current protocol.CodeIndexSettings
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		current, err = store.ViewCodeIndexSettings(ctx, db)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(saved, current) {
		t.Fatalf("refused save changed the stored settings from %+v to %+v", saved, current)
	}
}
