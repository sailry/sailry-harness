package client

import (
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderDirectoryReadOrderAndWatch(t *testing.T) {
	initial := projectionSnapshot(0)
	initial.ProviderDirectory = true
	subscription := testSubscription(initial)
	reads := make(chan chan link.Response, 2)
	transport := &clientTransport{subscriptions: []*clientSubscription{subscription}}
	transport.dispatch = func(request protocol.Request) (link.Admission, error) {
		if request.Command.Kind != "read_provider_directory" {
			return link.Admission{}, fault(protocol.ErrorCodeInvalidRequest, "unexpected full projection read")
		}
		completion := make(chan link.Response, 1)
		reads <- completion
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
	}
	controller := New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	changes := make(chan struct{}, 16)
	defer controller.OnChange(func() {
		select {
		case changes <- struct{}{}:
		default:
		}
	})()
	read := func() (chan link.Response, <-chan error) {
		t.Helper()
		done := make(chan error, 1)
		go func() { done <- controller.RefreshProviders(t.Context()) }()
		select {
		case completion := <-reads:
			return completion, done
		case err := <-done:
			t.Fatalf("directory read returned before dispatch: %v", err)
		case <-time.After(time.Second):
			t.Fatal("directory read was not dispatched")
		}
		return nil, nil
	}
	answer := func(completion chan link.Response, done <-chan error, cursor uint64, providers ...protocol.ConversationProvider) {
		t.Helper()
		completion <- link.Response{Output: protocol.Output{Kind: "provider_directory", Data: protocol.ProviderDirectory{Node: initial.Node, Cursor: cursor, Providers: providers}}}
		if err := <-done; err != nil {
			t.Fatal(err)
		}
	}
	model := protocol.DefaultConversationProvider()
	model.ID, model.Revision, model.Name = protocol.ProviderID{9}, 2, "Saved"
	old := model
	old.Name, old.Revision = "Old", 1
	first, firstDone := read()
	second, secondDone := read()
	answer(second, secondDone, 4, model)
	answer(first, firstDone, 1, old)
	if s := controller.Snapshot(); s.Cursor != 0 || len(s.Providers) != 1 || s.Providers[0].Name != "Saved" {
		t.Fatalf("late directory response rolled the projection back or skipped Node events: %+v", s)
	}
	third, thirdDone := read()
	subscription.updates <- projectionEvent(1, "provider_changed", old)
	subscription.updates <- projectionEvent(2, "defaults_changed", protocol.Defaults{Revision: 7})
	waitForCursor(t, controller, changes, 2)
	if s := controller.Snapshot(); len(s.Providers) != 1 || s.Providers[0].Name != "Saved" || s.Defaults.Revision != 7 {
		t.Fatalf("queued provider event replaced the directory or unrelated defaults were skipped: %+v", s)
	}
	olderSnapshot := initial
	olderSnapshot.Cursor, olderSnapshot.Providers, olderSnapshot.Defaults.Revision = 3, []protocol.ConversationProvider{old}, 7
	subscription.updates <- protocol.Update{Kind: "snapshot", Data: olderSnapshot}
	waitForCursor(t, controller, changes, 3)
	if s := controller.Snapshot(); len(s.Providers) != 1 || s.Providers[0].Name != "Saved" {
		t.Fatalf("older Node snapshot replaced the directory: %+v", s.Providers)
	}
	subscription.updates <- projectionEvent(4, "provider_changed", model)
	waitForCursor(t, controller, changes, 4)
	if s := controller.Snapshot(); len(s.Providers) != 1 || s.Providers[0].Name != "Saved" || s.Defaults.Revision != 7 {
		t.Fatalf("directory and Node watch did not converge: %+v", s)
	}
	subscription.updates <- projectionEvent(5, "provider_removed", protocol.EventProviderRemovedPayload{ID: model.ID})
	waitForCursor(t, controller, changes, 5)
	answer(third, thirdDone, 4, model)
	if len(controller.Snapshot().Providers) != 0 {
		t.Fatal("read predating removal revived the saved channel")
	}
}
