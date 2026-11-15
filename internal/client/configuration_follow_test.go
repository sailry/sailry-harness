package client

import (
	"context"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConfigurationFollowEventsKeepObservationAlive(t *testing.T) {
	t.Parallel()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	subscription := testSubscription(projectionSnapshot(0))
	transport := &clientTransport{subscriptions: []*clientSubscription{subscription}}
	controller := New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	changes := make(chan struct{}, 8)
	unsubscribe := controller.OnChange(func() {
		select {
		case changes <- struct{}{}:
		default:
		}
	})
	defer unsubscribe()
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	wait := func(cursor uint64) {
		t.Helper()
		for {
			select {
			case <-ctx.Done():
				t.Fatalf("follow policy event %d did not notify its observer: %v", cursor, context.Cause(ctx))
			case <-changes:
				if err := controller.Err(); err != nil {
					t.Fatalf("follow policy event ended observation: %v", err)
				}
				if controller.Snapshot().Cursor == cursor {
					return
				}
			}
		}
	}
	source := protocol.NodeID{2}
	// Target-owned policy events must keep notifying the source worker even
	// though the follow status is a feature read rather than a Snapshot field.
	for index, status := range []protocol.ConfigurationFollowStatus{
		{Revision: 1},
		{Revision: 2, Source: &source},
		{Revision: 2, Source: &source, MCPLocal: []protocol.ConfigurationMCPLocal{{Plugin: "fixture", Server: "server"}}},
	} {
		cursor := uint64(index + 1)
		subscription.updates <- projectionEvent(cursor, "configuration_follow_changed", status)
		wait(cursor)
	}
	subscription.updates <- projectionEvent(4, "power_settings_changed", protocol.EventPowerSettingsChangedPayload{Revision: 7})
	wait(4)
	if !controller.Connected() || controller.Snapshot().PowerSettingsRevision != 7 {
		t.Fatal("ordinary Node events stopped following a configuration policy change")
	}
	transport.mu.Lock()
	defer transport.mu.Unlock()
	if len(transport.requests) != 0 {
		t.Fatal("a feature-only invalidation requested an unrelated full snapshot")
	}
}
