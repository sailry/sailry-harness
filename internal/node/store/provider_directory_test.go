package store

import (
	"database/sql"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderDirectoryAndIncrementalEventsShareUserScope(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		local, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		other, err := CreateUser(ctx, db, "Other")
		if err != nil {
			return err
		}
		ownerCtx := WithUser(ctx, local)
		otherCtx := WithUser(ctx, other)
		provider := providerFixture()
		cursor, err := EventCursor(ctx, db)
		if err != nil {
			return err
		}
		_, changes, err := ExecuteProviderCommand(ownerCtx, db, node, protocol.NewRequest(node, protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: provider}}), nil)
		if err != nil {
			return err
		}
		changes = append(changes, protocol.Event{Kind: "provider_quota_changed", Data: protocol.EventProviderQuotaChangedPayload{Provider: provider.ID}})
		events, err := RecordEvents(ownerCtx, db, node, changes)
		if err != nil {
			return err
		}
		for _, own := range []bool{true, false} {
			caller := otherCtx
			if own {
				caller = ownerCtx
			}
			out, err := ReadProviderCommand(caller, db, node, protocol.Command{Kind: "read_provider_directory"})
			if err != nil {
				return err
			}
			directory := out.Data.(protocol.ProviderDirectory)
			if out.Kind != "provider_directory" || directory.Node != node || directory.Cursor != events[1].Cursor {
				t.Fatalf("incorrect directory for owner=%t: %+v", own, directory)
			}
			if own && (len(directory.Providers) != 1 || directory.Providers[0].ID != provider.ID) || !own && len(directory.Providers) != 0 {
				t.Fatalf("incorrect visible providers for owner=%t: %+v", own, directory.Providers)
			}
			_, observed, err := EventPage(caller, db, node, cursor, directory.Cursor)
			if err != nil {
				return err
			}
			if len(observed) != len(changes) {
				t.Fatalf("provider events for owner=%t: %v", own, observed)
			}
			for i, envelope := range observed {
				want := "cursor"
				if own {
					want = changes[i].Kind
				}
				if envelope.Event.Kind != want || envelope.Cursor != events[i].Cursor {
					t.Fatalf("owner=%t got %s at %d, want %s at %d", own, envelope.Event.Kind, envelope.Cursor, want, events[i].Cursor)
				}
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
