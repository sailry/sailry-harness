package node

import (
	"context"
	"database/sql"
	"errors"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSaveNetworkAppliesBeforeStoring(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	ctx := context.Background()
	previous := protocol.NetworkSelection{Kind: "direct"}
	custom := func(relay string) protocol.NetworkSelection {
		return protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{relay}}}
	}
	stored := func() (uint64, *protocol.NetworkSelection) {
		t.Helper()
		var revision uint64
		var selection *protocol.NetworkSelection
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			revision, selection, err = store.ReadNetworkSelection(ctx, db)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		return revision, selection
	}
	var applied []string
	apply := func(failure error) func(context.Context, protocol.NetworkSelection) error {
		return func(_ context.Context, selection protocol.NetworkSelection) error {
			applied = append(applied, selection.Kind)
			return failure
		}
	}
	write := func(statement string) {
		t.Helper()
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			_, err := db.ExecContext(ctx, statement)
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}

	t.Run("a selection that could not be applied is not stored", func(t *testing.T) {
		applied = nil
		refused := &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "relay did not answer"}
		if err := node.saveNetwork(ctx, 0, previous, custom("/dns4/relay.example/tcp/443/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV"), apply(refused)); !errors.Is(err, refused) {
			t.Fatalf("save = %v", err)
		}
		if revision, selection := stored(); revision != 0 || selection != nil {
			t.Fatalf("a selection that failed would take effect at the next start: revision %d, %#v", revision, selection)
		}
		if len(applied) != 1 {
			t.Fatalf("applied = %v", applied)
		}
	})
	t.Run("a stale revision changes no relay", func(t *testing.T) {
		applied = nil
		err := node.saveNetwork(ctx, 3, previous, custom("/dns4/relay.example/tcp/443/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV"), apply(nil))
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeRevisionConflict {
			t.Fatalf("save = %v", err)
		}
		if len(applied) != 0 {
			t.Fatalf("relays changed for a stale revision: %v", applied)
		}
	})
	t.Run("a selection that could not be stored is taken back", func(t *testing.T) {
		applied = nil
		write("CREATE TEMP TRIGGER network_refused BEFORE INSERT ON network_settings BEGIN SELECT RAISE(ABORT,'network write refused'); END")
		defer write("DROP TRIGGER network_refused")
		if err := node.saveNetwork(ctx, 0, previous, custom("/dns4/relay.example/tcp/443/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV"), apply(nil)); err == nil {
			t.Fatal("an unstored selection was reported saved")
		}
		if len(applied) != 2 || applied[0] != "custom" || applied[1] != "direct" {
			t.Fatalf("applied = %v", applied)
		}
		if revision, selection := stored(); revision != 0 || selection != nil {
			t.Fatalf("stored revision %d, %#v", revision, selection)
		}
	})
	t.Run("an applied selection is stored", func(t *testing.T) {
		applied = nil
		if err := node.saveNetwork(ctx, 0, previous, custom("/dns4/relay.example/tcp/443/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV"), apply(nil)); err != nil {
			t.Fatal(err)
		}
		revision, selection := stored()
		if revision != 1 || selection == nil || selection.Kind != "custom" || len(applied) != 1 {
			t.Fatalf("stored revision %d, %#v, applied %v", revision, selection, applied)
		}
	})
}

func TestRetiredIrohSelectionDoesNotBlockNetworkStartup(t *testing.T) {
	for _, retired := range []protocol.NetworkSelection{
		{Kind: "iroh"},
		{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{"https://old-relay.example/"}}},
	} {
		t.Run(retired.Kind, func(t *testing.T) {
			node := nodeFixture(t)
			ctx := t.Context()
			if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
				_, err := store.SaveNetworkSelection(ctx, db, 0, retired)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			if err := node.bindNetwork(ctx, link.NetworkScope{Internet: true, Bind: "127.0.0.1:0"}, ""); err != nil {
				t.Fatal(err)
			}
			settings, err := node.readNetwork(ctx)
			if err != nil || settings.Revision != 1 || settings.Selection == nil || settings.Effective == nil ||
				settings.Selection.Kind != "direct" || settings.Effective.Kind != "direct" {
				t.Fatalf("settings after retiring old relay = %#v, %v", settings, err)
			}
			if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				revision, saved, err := store.ReadNetworkSelection(ctx, db)
				if err == nil && (revision != 1 || saved == nil || saved.Kind != retired.Kind) {
					t.Errorf("stored selection changed: %d, %#v", revision, saved)
				}
				return err
			}); err != nil {
				t.Fatal(err)
			}
		})
	}
}
