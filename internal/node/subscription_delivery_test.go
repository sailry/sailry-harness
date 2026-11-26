package node

import (
	"context"
	"database/sql"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSubscriptionReadsOnlyTheDeliveredCursorWithCurrentAccess(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	ctx := t.Context()
	caller := protocol.NodeID{37}
	write := func(f func(*sql.Conn) error) {
		t.Helper()
		if err := node.profile.Store.Node.Write(ctx, f); err != nil {
			t.Fatal(err)
		}
	}
	write(func(db *sql.Conn) error { return store.PairLinkPeer(ctx, db, caller, link.DirectionController) })
	stream, err := node.Subscribe(ctx, caller, protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer stream.Close()
	initial, err := stream.Next(ctx)
	if err != nil || initial.Kind != "snapshot" {
		t.Fatalf("initial update = %+v, %v", initial, err)
	}
	project := protocol.NewProjectID()
	var recorded []protocol.EventEnvelope
	write(func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, `INSERT INTO projects(id,name,path,appearance,visibility) VALUES(?1,'Delivery','/delivery','{}','shared')`, project.String()); err != nil {
			return err
		}
		event := protocol.Event{Kind: "plugin_values_changed", Data: protocol.EventPluginValuesChangedPayload{Name: "fixture", Project: &project}}
		var err error
		recorded, err = store.RecordEvents(ctx, db, node.ID(), []protocol.Event{event, event, event})
		if err != nil {
			return err
		}
		// An unreadable future audience must not poison the two valid deliveries before it.
		_, err = db.ExecContext(ctx, "UPDATE events SET audience=?1 WHERE cursor=?2", []byte("{"), recorded[2].Cursor)
		return err
	})
	node.publish()
	next := func(index int, kind string) {
		t.Helper()
		update, err := stream.Next(ctx)
		event, ok := update.Data.(protocol.EventEnvelope)
		if err != nil || update.Kind != "event" || !ok || event.Cursor != recorded[index].Cursor || event.Event.Kind != kind || kind == "cursor" && event.Event.Data != nil {
			t.Fatalf("delivery %d = %+v, %v; want %s", index, update, err, kind)
		}
	}
	next(0, "plugin_values_changed")
	write(func(db *sql.Conn) error {
		_, err := db.ExecContext(ctx, "UPDATE projects SET visibility='private' WHERE id=?1", project.String())
		return err
	})
	next(1, "cursor") // No publish hint: delivery must independently observe the commit.
	if update, err := stream.Next(ctx); err != nil || update.Kind != "reset_required" || update.Data != nil {
		t.Fatalf("malformed audience disclosed an event or did not reset: %+v, %v", update, err)
	}
	topic := protocol.Topic{Kind: "node"}
	verified, err := node.authorizeTopic(ctx, caller, topic)
	if err != nil {
		t.Fatal(err)
	}
	// Revoke after the precheck, while the source is producing an otherwise valid update.
	source := deliverySource(func(ctx context.Context) (protocol.Update, error) {
		err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error { return store.SetLinkTrust(ctx, db, caller, false) })
		return initial, err
	})
	guarded := node.trackSubscription(verified, caller, topic, source)
	defer guarded.Close()
	if update, err := guarded.Next(ctx); !pluginMessageFault(err, protocol.ErrorCodePermissionDenied) || update.Data != nil {
		t.Fatalf("revoked controller received an in-flight update: %+v, %v", update, err)
	}
}

func TestAuthorizedSubscriptionSkipsRedundantChecksUntilDatabaseWrites(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	// Only the test writes to this profile, so each invalidation has a known cause.
	profile, err := OpenProfile(ctx, filepath.Join(t.TempDir(), "profile"))
	if err != nil {
		t.Fatal(err)
	}
	defer profile.Close()
	node := &Node{profile: profile}
	caller := protocol.NodeID{38}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		return store.PairLinkPeer(ctx, db, caller, link.DirectionController)
	}); err != nil {
		t.Fatal(err)
	}

	topic := protocol.Topic{Kind: "node"}
	verified, err := node.authorizeTopic(ctx, caller, topic)
	if err != nil {
		t.Fatal(err)
	}
	var checks int
	rawCheck := func(next context.Context) error {
		checks++
		_, err := node.authorizeTopic(store.WithUser(next, userFromContext(verified)), caller, topic)
		return err
	}
	delivered := 0
	source := deliverySource(func(ctx context.Context) (protocol.Update, error) {
		delivered++
		if delivered == 4 {
			if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
				return store.SetLinkTrust(ctx, db, caller, false)
			}); err != nil {
				return protocol.Update{}, err
			}
		}
		return protocol.Update{Kind: "reset_required"}, nil
	})
	guarded := &authorizedSubscription{accessObservation: node.observeAccess(rawCheck, source.Close), source: source}
	defer guarded.Close()

	// Three deliveries with no intervening DB writes perform only the initial check.
	for i := range 3 {
		if _, err := guarded.Next(ctx); err != nil {
			t.Fatalf("delivery %d failed: %v", i, err)
		}
	}
	if checks != 1 {
		t.Fatalf("unchanged DB ran %d access checks across 3 deliveries, want 1", checks)
	}
	// Direct DB revocation during the 4th delivery advances WriteSequence and
	// forces an immediate re-check without publishAccess.
	if _, err := guarded.Next(ctx); !pluginMessageFault(err, protocol.ErrorCodePermissionDenied) {
		t.Fatalf("revoked delivery returned %v, want PermissionDenied", err)
	}
	if checks != 2 {
		t.Fatalf("revocation ran %d total checks, want 2", checks)
	}
}

type deliverySource func(context.Context) (protocol.Update, error)

func (source deliverySource) Next(ctx context.Context) (protocol.Update, error) { return source(ctx) }
func (source deliverySource) Close() error                                      { return nil }
