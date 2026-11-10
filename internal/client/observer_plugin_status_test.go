package client

import (
	"context"
	"encoding/json"
	"reflect"
	"strings"
	"sync"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginStatusTestStream struct {
	updates chan protocol.Update
	closed  chan struct{}
	once    sync.Once
}

func (stream *pluginStatusTestStream) Next(ctx context.Context) (protocol.Update, error) {
	select {
	case update := <-stream.updates:
		return update, nil
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case <-stream.closed:
		return protocol.Update{}, fault(protocol.ErrorCodeUnavailable, "Status fixture disconnected")
	}
}

func (stream *pluginStatusTestStream) Close() error {
	stream.once.Do(func() { close(stream.closed) })
	return nil
}

type pluginStatusTestTransport struct {
	*clientTransport
	streams []*pluginStatusTestStream
	calls   []protocol.Topic
}

func (transport *pluginStatusTestTransport) Subscribe(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	transport.calls = append(transport.calls, copyValue(topic))
	if len(transport.streams) == 0 {
		return nil, fault(protocol.ErrorCodeUnavailable, "Status fixture has no subscription")
	}
	stream := transport.streams[0]
	transport.streams = transport.streams[1:]
	return stream, nil
}

func testPluginStatusObserver(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		first := &pluginStatusTestStream{updates: make(chan protocol.Update, 4), closed: make(chan struct{})}
		second := &pluginStatusTestStream{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
		transport := &pluginStatusTestTransport{clientTransport: &clientTransport{}, streams: []*pluginStatusTestStream{first, second}}
		controller := New(transport)
		worktree := protocol.NewWorktreeID()
		scope := protocol.PluginUiStatusScope{Worktree: &worktree}
		observer := controller.watchStatusView(t.Context(), scope)
		defer func() { _ = observer.Close(); _ = controller.Close() }()
		entry := pluginStatusTestEntry("z-package", "badge", 2)
		ordered := pluginStatusTestEntry("a-package", "first", 1)
		invalid := pluginStatusTestEntry("invalid", "bad", 0)
		invalid.State.Value = json.RawMessage("true")
		snapshot := protocol.PluginUiStatusSnapshot{
			Node: controller.Target(), Scope: scope, Entries: []protocol.PluginUiStatusEntry{entry, invalid, ordered},
			Failures: []protocol.PluginUiStatusFailure{},
		}
		first.updates <- protocol.Update{Kind: "status", Data: snapshot}
		readStatus := func() StatusView {
			t.Helper()
			body, err := observer.Next(t.Context())
			if err != nil {
				t.Fatal(err)
			}
			var view StatusView
			if err := json.Unmarshal(body, &view); err != nil {
				t.Fatal(err)
			}
			return view
		}
		view := readStatus()
		if !view.Connected || view.Error != nil || len(view.Entries) != 2 ||
			view.Entries[0].Package.Name != ordered.Package.Name || view.Entries[1].Package.Name != entry.Package.Name {
			t.Fatal("Status feed did not filter, order and connect its captured entries")
		}
		wrong := snapshot
		wrong.Node = protocol.NodeID{9}
		first.updates <- protocol.Update{Kind: "status", Data: wrong}
		view = readStatus()
		if view.Connected || view.Error == nil || view.Error.Code != protocol.ErrorCodeWrongTarget || len(view.Entries) != 2 {
			t.Fatal("Status target failure lost retained entries or remained connected")
		}
		time.Sleep(499 * time.Millisecond)
		synctest.Wait()
		transport.mu.Lock()
		count := len(transport.calls)
		transport.mu.Unlock()
		if count != 1 {
			t.Fatal("Status feed retried before its source delay")
		}
		time.Sleep(time.Millisecond)
		synctest.Wait()
		transport.mu.Lock()
		count = len(transport.calls)
		transport.mu.Unlock()
		if count != 2 {
			t.Fatal("Status feed did not retry after 500 milliseconds")
		}
		second.updates <- protocol.Update{Kind: "status", Data: snapshot}
		if view := readStatus(); !view.Connected || view.Error != nil || len(view.Entries) != 2 {
			t.Fatal("Status feed did not recover its retained state")
		}
		request, err := controller.PrepareStatus(scope, entry)
		if err != nil || !request.Command.Durable() || request.Command.Kind != "call_plugin" ||
			request.Plugin == nil || request.Plugin.Package != entry.Package || !reflect.DeepEqual(request.Plugin.Worktree, scope.Worktree) {
			t.Fatalf("Status button did not capture its exact durable request: %v", err)
		}
		captured := *request.Plugin.Worktree
		worktree = protocol.NewWorktreeID()
		if *request.Plugin.Worktree != captured {
			t.Fatal("Status request followed caller-owned resource mutation")
		}
		if _, err := controller.PrepareStatus(protocol.PluginUiStatusScope{Session: new(protocol.NewSessionID())}, entry); faultCode(err) != protocol.ErrorCodeInvalidRequest {
			t.Fatal("Status session action omitted its resolved worktree")
		}
		invalid = entry
		invalid.State.ReplyTo = new(uint64(1))
		if _, err := controller.PrepareStatus(scope, invalid); faultCode(err) != protocol.ErrorCodeInvalidRequest {
			t.Fatal("Status button skipped full shared state validation")
		}
		if err := controller.Close(); err != nil {
			t.Fatal(err)
		}
		select {
		case <-second.closed:
		default:
			t.Fatal("Client close did not join and release its Status subscription")
		}
		controller.pluginStatusObservers.mu.Lock()
		remaining := len(controller.pluginStatusObservers.members)
		controller.pluginStatusObservers.mu.Unlock()
		if remaining != 0 {
			t.Fatal("Client close retained Status observer owners")
		}
	})
}

func pluginStatusTestEntry(name, id string, order int16) protocol.PluginUiStatusEntry {
	declaration := protocol.DefaultPluginUiContribution()
	declaration.ID, declaration.Order = id, order
	declaration.Slot, declaration.Kind = protocol.PluginUiSlotStatus, protocol.PluginUiKindButton
	declaration.Label.Label, declaration.Handler = id, new("click")
	state := protocol.DefaultPluginUiState()
	state.ID, state.Visible, state.Enabled = id, true, true
	return protocol.PluginUiStatusEntry{
		Package:     protocol.PluginReference{Name: name, Digest: strings.Repeat("a", 64)},
		Declaration: declaration, State: state, Invoke: new("invoke"),
	}
}
