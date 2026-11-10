package node

import (
	"context"
	"reflect"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
)

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
