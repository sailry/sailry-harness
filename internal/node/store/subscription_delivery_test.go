package store

import (
	"database/sql"
	"fmt"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestEventMemberVisibilityKeepsCapturedAudienceAndMissingProjectOwner(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		user, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		other, err := CreateUser(ctx, db, "Other owner")
		if err != nil {
			return err
		}
		merged, err := CreateUser(ctx, db, "Earlier owner")
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "UPDATE users SET state='merged',merged_into=?1 WHERE id=?2", user.ID.String(), merged.ID.String()); err != nil {
			return err
		}
		shared, missing := protocol.NewProjectID(), protocol.NewProjectID()
		if _, err := db.ExecContext(ctx, `INSERT INTO projects(id,name,path,appearance,owner,visibility) VALUES(?1,'Delivery','/delivery','{}',?2,'shared')`, shared.String(), other.ID.String()); err != nil {
			return err
		}
		audience := eventAudience{Kind: "members", Members: map[string]eventAudience{}}
		var ids []protocol.SessionID
		for _, scope := range []eventAudience{
			{Kind: "project", Project: &shared, Owner: &other.ID, Shared: true},
			{Kind: "project", Project: &shared, Owner: &other.ID},
			{Kind: "project", Project: &missing, Owner: &merged.ID, Shared: true},
			{Kind: "project", Project: &missing, Owner: &other.ID, Shared: true},
			{Kind: "user", Owner: &merged.ID},
		} {
			id := protocol.NewSessionID()
			ids = append(ids, id)
			audience.Members[id.String()] = scope
		}
		ids = append(ids, ids[0])
		filtered, err := filterEvent(ctx, db, protocol.Event{Kind: "sessions_reordered", Data: ids}, audience, user)
		if err != nil {
			return err
		}
		visible, ok := filtered.Data.([]protocol.SessionID)
		if !ok || !slices.Equal(visible, []protocol.SessionID{ids[0], ids[2], ids[4], ids[0]}) {
			return fmt.Errorf("mixed captured audiences lost visibility, order or duplicate IDs: %+v", filtered)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func BenchmarkSubscriptionEventDelivery(b *testing.B) {
	_, node, storage := schemaFixture(b)
	ctx := b.Context()
	const count = 128
	var after uint64
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		after, err = EventCursor(ctx, db)
		if err != nil {
			return err
		}
		events := make([]protocol.Event, count)
		for i := range events {
			events[i] = protocol.Event{Kind: "power_settings_changed", Data: protocol.EventPowerSettingsChangedPayload{Revision: uint64(i + 1)}}
		}
		_, err = RecordEvents(ctx, db, node, events)
		return err
	}); err != nil {
		b.Fatal(err)
	}
	for _, path := range []string{"page_then_refresh", "at_delivery"} {
		b.Run(path, func(b *testing.B) {
			b.ReportAllocs()
			ctx := b.Context()
			var reads, filtered int
			for b.Loop() {
				reads, filtered = 0, 0
				var page []protocol.EventEnvelope
				if path == "page_then_refresh" {
					if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
						var err error
						_, page, err = EventPage(ctx, db, node, after, 0)
						return err
					}); err != nil || len(page) != count {
						b.Fatalf("page length = %d, %v", len(page), err)
					}
					reads++
					filtered += len(page)
				}
				var through uint64
				for i := range count {
					cursor := after + uint64(i)
					if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
						var event *protocol.EventEnvelope
						var err error
						if page == nil {
							through, event, err = NextEvent(ctx, db, node, cursor, through)
						} else {
							var refreshed protocol.EventEnvelope
							refreshed, err = RefreshEventAccess(ctx, db, page[i])
							event = &refreshed
						}
						if err == nil && (event == nil || event.Cursor != cursor+1 || event.Event.Kind != "power_settings_changed") {
							return fmt.Errorf("unexpected delivery for cursor %d: %+v", cursor+1, event)
						}
						return err
					}); err != nil {
						b.Fatal(err)
					}
					reads++
					filtered++
				}
			}
			b.ReportMetric(float64(reads), "transactions/burst")
			b.ReportMetric(float64(filtered), "filtered-rows/burst")
		})
	}
}
