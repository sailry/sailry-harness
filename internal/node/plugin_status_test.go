package node

import (
	"context"
	"database/sql"
	"reflect"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Over 1 s: the Node start installs and verifies the bundled packages.
func TestPluginStatusStartsFromPinnedEventCursor(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	ctx := t.Context()
	caller := protocol.NodeID{31}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		return store.PairLinkPeer(ctx, db, caller, link.DirectionController)
	}); err != nil {
		t.Fatal(err)
	}
	send := func(command protocol.Command) protocol.Output {
		t.Helper()
		result := dispatchFixture(t, node, caller, protocol.NewRequest(node.ID(), command))
		if result.Fault != nil {
			t.Fatal(result.Fault)
		}
		return result.Output
	}
	config := sessionConfigFixtureFor(t, node, caller)
	create := protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &config}}
	session := send(create).Data.(protocol.Session)
	unrelated := send(create).Data.(protocol.Session)
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(ctx, "UPDATE session_revisions SET config=?1 WHERE session=?2 AND revision=?3", []byte("{"), unrelated.ID.String(), unrelated.Revision)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if full, err := node.Subscribe(ctx, caller, protocol.Topic{Kind: "node"}); err == nil {
		_ = full.Close()
		t.Fatal("the full snapshot did not encounter the malformed unrelated session")
	} else if !pluginMessageFault(err, protocol.ErrorCodeInternal) {
		t.Fatalf("full snapshot failed for an unexpected reason: %v", err)
	}

	scope := protocol.PluginUiStatusScope{Session: &session.ID}
	status, err := node.Subscribe(ctx, caller, protocol.Topic{Kind: "status", Data: scope})
	if err != nil {
		t.Fatal(err)
	}
	defer status.Close()
	wait, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	nextStatus := func() protocol.PluginUiStatusSnapshot {
		t.Helper()
		update, err := status.Next(wait)
		snapshot, ok := update.Data.(protocol.PluginUiStatusSnapshot)
		if err != nil || update.Kind != "status" || !ok || snapshot.Node != node.ID() || !reflect.DeepEqual(snapshot.Scope, scope) {
			t.Fatalf("scoped Status update = %#v, %v", update, err)
		}
		return snapshot
	}
	nextStatus()
	// Start the status event source without a reader, then commit before its first Next.
	source, err := node.subscribePluginStatusEvents(ctx, caller)
	if err != nil {
		t.Fatal(err)
	}
	defer source.Close()
	send(protocol.Command{Kind: "remove_session", Data: protocol.CommandRemoveSessionPayload{Session: session.ID, ExpectedRevision: session.Revision}})
	update, err := source.Next(wait)
	envelope, ok := update.Data.(protocol.EventEnvelope)
	removed, _ := envelope.Event.Data.([]protocol.SessionID)
	if err != nil || update.Kind != "event" || !ok || envelope.Event.Kind != "sessions_removed" || len(removed) != 1 || removed[0] != session.ID {
		t.Fatalf("first event after reserving the Status source = %#v, %v", update, err)
	}
	if final := nextStatus(); len(final.Entries) != 0 {
		t.Fatalf("removed session retained Status entries: %#v", final.Entries)
	}
	if _, err := status.Next(wait); !pluginMessageFault(err, protocol.ErrorCodeNotFound) {
		t.Fatalf("removed session did not terminate Status: %v", err)
	}
}

func testPluginStatusScheduling(t *testing.T) {
	clock := clockwork.NewFakeClockAt(time.Unix(1000, 0))
	now := clock.Now()
	dirty := pluginStatusDirty{first: now, latest: now}
	if dirty.due(time.Time{}) != now.Add(pluginStatusDebounce) {
		t.Fatal("Status debounce changed")
	}
	clock.Advance(600 * time.Millisecond)
	dirty.latest = clock.Now()
	if dirty.due(time.Time{}) != now.Add(pluginStatusMaximumWait) || dirty.due(now.Add(pluginStatusFailureWait)) != now.Add(pluginStatusFailureWait) {
		t.Fatal("Status dirty window or failure cooldown changed")
	}
	dirty.force = true
	if dirty.due(now.Add(pluginStatusFailureWait)) != clock.Now() {
		t.Fatal("forced Status refresh did not bypass cooldown")
	}
	state := protocol.DefaultPluginUiState()
	state.ID = "badge"
	declaration := protocol.DefaultPluginUiContribution()
	declaration.ID, declaration.Slot, declaration.Kind, declaration.Label.Label = "badge", protocol.PluginUiSlotStatus, protocol.PluginUiKindButton, "Badge"
	entry := protocol.PluginUiStatusEntry{Declaration: declaration, State: state}
	expected := []protocol.PluginUiStatusEntry{entry}
	owner := &pluginStatusPackage{entries: expected, running: true}
	busy := &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "Status fixture is busy"}
	if owner.complete(nil, busy, now) != nil || !reflect.DeepEqual(owner.entries, expected) {
		t.Fatal("first Busy Status cycle lost retained entries or reported early")
	}
	if deadline, pending := owner.deadline(); !pending || deadline != now.Add(pluginStatusFailureWait) {
		t.Fatal("first Busy Status cycle did not schedule exactly its delayed retry")
	}
	owner.mark(now.Add(time.Second), false)
	if deadline, _ := owner.deadline(); deadline != now.Add(pluginStatusFailureWait) {
		t.Fatal("ordinary invalidation bypassed failure cooldown")
	}
	owner.running, owner.dirty = true, nil
	if owner.complete(nil, busy, now.Add(pluginStatusFailureWait)) == nil {
		t.Fatal("second Busy Status cycle did not report its failure")
	}
	if _, pending := owner.deadline(); pending {
		t.Fatal("second Busy Status cycle invented a periodic retry")
	}
	if owner.complete(nil, busy, now.Add(pluginStatusFailureWait)) != nil || owner.complete(expected, nil, now) != nil ||
		owner.failure != nil || owner.reported || !owner.retryAfter.IsZero() {
		t.Fatal("Status failure reporting did not reset after success")
	}
	owner.running = true
	owner.mark(now, false)
	if _, pending := owner.deadline(); pending {
		t.Fatal("Status launched concurrent evaluations for one package")
	}
	if owner.complete(expected, nil, now) != nil {
		t.Fatal("successful Status completion failed")
	}
	if _, pending := owner.deadline(); !pending {
		t.Fatal("Status invalidation during evaluation was lost")
	}
	private := &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "Status fixture failed", Details: &protocol.FaultDetails{Kind: "system_diagnostic"}}
	public := pluginStatusPublicFault(private)
	if public.Code != private.Code || public.Message != private.Message || public.Details != nil {
		t.Fatal("Status public failure retained attributed diagnostic details")
	}
	watcher := &pluginStatusWatcher{
		service: &pluginStatusService{node: &Node{closed: make(chan struct{})}},
		ctx:     context.Background(), changed: make(chan struct{}), wake: make(chan struct{}, 1),
	}
	snapshot := protocol.PluginUiStatusSnapshot{Node: protocol.NodeID{1}, Entries: expected, Failures: []protocol.PluginUiStatusFailure{}}
	watcher.publish(pluginStatusFrame{snapshot: snapshot})
	revision := watcher.revision
	watcher.publish(pluginStatusFrame{snapshot: snapshot})
	if watcher.revision != revision {
		t.Fatal("identical Status snapshots were republished")
	}
	feed := &pluginStatusFeed{watcher: watcher, closed: make(chan struct{})}
	update, err := feed.Next(t.Context())
	if err != nil || update.Kind != "status" {
		t.Fatalf("Status feed did not publish its initial snapshot: %v", err)
	}
	read := update.Data.(protocol.PluginUiStatusSnapshot)
	read.Entries[0].State.ID = "mutated"
	if watcher.frame.snapshot.Entries[0].State.ID != "badge" {
		t.Fatal("Status subscriber mutated the retained snapshot")
	}
	terminal := &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "status session was removed"}
	watcher.publish(pluginStatusFrame{snapshot: protocol.PluginUiStatusSnapshot{Node: snapshot.Node, Entries: []protocol.PluginUiStatusEntry{}, Failures: []protocol.PluginUiStatusFailure{}}, terminal: terminal})
	update, err = feed.Next(t.Context())
	if err != nil || len(update.Data.(protocol.PluginUiStatusSnapshot).Entries) != 0 {
		t.Fatalf("Status terminal feed did not first publish its final empty snapshot: %v", err)
	}
	if _, err := feed.Next(t.Context()); err == nil || pluginStatusPublicFault(err).Code != protocol.ErrorCodeNotFound {
		t.Fatal("Status terminal fault did not follow the final snapshot")
	}
}
