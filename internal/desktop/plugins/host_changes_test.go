package plugins

import (
	"context"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testHostConstructionObservations(t *testing.T) {
	t.Run("initial refresh and Client callbacks serialize", func(t *testing.T) {
		ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
		defer cancel()
		plugin := protocol.DefaultPluginSummary()
		plugin.Name, plugin.Digest = "construction-fixture", strings.Repeat("a", 64)
		plugin.Revision, plugin.SettingsRevision, plugin.Enabled = 1, 1, true
		initial := protocol.DefaultSnapshot()
		initial.Node, initial.Cursor, initial.Plugins = protocol.NodeID{1}, 10, []protocol.PluginSummary{plugin}
		refreshed := initial
		plugin.Revision, plugin.SettingsRevision = 2, 2
		refreshed.Cursor, refreshed.Plugins = 11, []protocol.PluginSummary{plugin}
		first, second := newHostConstructionSubscription(initial), newHostConstructionSubscription(refreshed)
		transport := &hostConstructionTransport{
			hostTestTransport: hostTestTransport{target: initial.Node},
			subscriptions:     make(chan *hostConstructionSubscription, 2),
		}
		transport.subscriptions <- first
		transport.subscriptions <- second
		controller := client.New(transport)
		defer controller.Close()
		if err := controller.Start(ctx); err != nil {
			t.Fatal(err)
		}
		clock := &hostConstructionClock{
			Clock: clockwork.NewFakeClock(), observing: make(chan struct{}), resume: make(chan struct{}),
		}
		defer clock.unblock()
		scope := protocol.DefaultPluginContext()
		scope.Package = protocol.PluginReference{Name: plugin.Name, Digest: plugin.Digest, SettingsRevision: 1}
		scope.Surface.Kind = "settings"
		constructed := make(chan *Host)
		go func() {
			owner := NewHost(HostBinding{Context: ctx, Client: controller, Scope: scope, Clock: clock})
			select {
			case constructed <- owner:
			case <-ctx.Done():
				_ = owner.Close()
			}
		}()
		select {
		case <-clock.observing:
		case owner := <-constructed:
			_ = owner.Close()
			t.Fatal("initial Host state bypassed the synchronized observation path")
		case <-ctx.Done():
			t.Fatal("initial Host observation did not start")
		}
		refreshing := make(chan error, 1)
		go func() { refreshing <- controller.Refresh(ctx) }()
		select {
		case <-second.reading:
		case <-ctx.Done():
			t.Fatal("Client refresh did not apply its replacement snapshot")
		}
		clock.unblock()
		var owner *Host
		select {
		case owner = <-constructed:
			defer owner.Close()
		case <-ctx.Done():
			t.Fatal("Host construction did not finish")
		}
		select {
		case err := <-refreshing:
			if err != nil {
				t.Fatal(err)
			}
		case <-ctx.Done():
			t.Fatal("Client callback did not finish")
		}
		cursors := controller.PluginCursors(plugin.Name, nil, nil)
		owner.changes.mu.Lock()
		state := owner.changes.state
		owner.changes.mu.Unlock()
		if !state.connected || !state.visible || state.cursors != [4]uint64{cursors.Cursor, cursors.Package, cursors.Settled, cursors.Settled} ||
			state.terminalTools != cursors.TerminalTools || state.updated[0].IsZero() {
			t.Fatalf("Host retained incomplete or stale observation state: %+v", state)
		}
		owner.mu.Lock()
		settings := owner.settings
		owner.mu.Unlock()
		if settings.invalid || !settings.connected || settings.current.SettingsRevision != 2 || owner.ctx.Err() != nil {
			t.Fatalf("Host settings lost the concurrent Client refresh: %+v %v", settings, owner.ctx.Err())
		}
	})
	t.Run("settings remain pending before the first connection", func(t *testing.T) {
		controller := client.New(&hostTestTransport{target: protocol.NodeID{1}})
		defer controller.Close()
		scope := protocol.DefaultPluginContext()
		scope.Package.Name, scope.Surface.Kind = "construction-fixture", "settings"
		owner := NewHost(HostBinding{Context: t.Context(), Client: controller, Scope: scope})
		defer owner.Close()
		if owner.ctx.Err() != nil || owner.settings.invalid || owner.settings.connected {
			t.Fatal("initial disconnected settings were treated as a lost connection")
		}
	})
}

type hostConstructionClock struct {
	clockwork.Clock
	observing chan struct{}
	resume    chan struct{}
	once      sync.Once
	released  sync.Once
}

func (clock *hostConstructionClock) Now() time.Time {
	clock.once.Do(func() {
		close(clock.observing)
		<-clock.resume
	})
	return clock.Clock.Now()
}

func (clock *hostConstructionClock) unblock() {
	clock.released.Do(func() { close(clock.resume) })
}

type hostConstructionTransport struct {
	hostTestTransport
	subscriptions chan *hostConstructionSubscription
}

func (transport *hostConstructionTransport) Subscribe(ctx context.Context, _ protocol.Topic) (link.Subscription, error) {
	select {
	case subscription := <-transport.subscriptions:
		return subscription, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

type hostConstructionSubscription struct {
	initial *protocol.Snapshot
	reading chan struct{}
	closed  chan struct{}
	ready   sync.Once
	close   sync.Once
}

func newHostConstructionSubscription(snapshot protocol.Snapshot) *hostConstructionSubscription {
	return &hostConstructionSubscription{initial: &snapshot, reading: make(chan struct{}), closed: make(chan struct{})}
}

func (subscription *hostConstructionSubscription) Next(ctx context.Context) (protocol.Update, error) {
	if subscription.initial != nil {
		snapshot := *subscription.initial
		subscription.initial = nil
		return protocol.Update{Kind: "snapshot", Data: snapshot}, nil
	}
	subscription.ready.Do(func() { close(subscription.reading) })
	select {
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case <-subscription.closed:
		return protocol.Update{}, context.Canceled
	}
}

func (subscription *hostConstructionSubscription) Close() error {
	subscription.close.Do(func() { close(subscription.closed) })
	return nil
}
