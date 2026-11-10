package client

import (
	"cmp"
	"context"
	"encoding/json"
	"reflect"
	"slices"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type StatusView struct {
	Entries   []protocol.PluginUiStatusEntry   `json:"entries"`
	Failures  []protocol.PluginUiStatusFailure `json:"failures"`
	Connected bool                             `json:"connected"`
	Error     *protocol.Fault                  `json:"error"`
}

type pluginStatusObservers struct {
	mu      sync.Mutex
	members map[*Observer]struct{}
	closed  bool
	once    sync.Once
	done    chan struct{}
	owners  sync.WaitGroup
}

func newPluginStatusObservers() *pluginStatusObservers {
	return &pluginStatusObservers{members: map[*Observer]struct{}{}, done: make(chan struct{})}
}

func (owners *pluginStatusObservers) add(observer *Observer) bool {
	owners.mu.Lock()
	defer owners.mu.Unlock()
	if owners.closed {
		return false
	}
	owners.members[observer] = struct{}{}
	owners.owners.Add(1)
	return true
}

func (owners *pluginStatusObservers) remove(observer *Observer) {
	owners.mu.Lock()
	delete(owners.members, observer)
	owners.mu.Unlock()
}

func (client *Client) closePluginStatusObservers() {
	owners := client.pluginStatusObservers
	if owners == nil {
		return
	}
	owners.once.Do(func() {
		owners.mu.Lock()
		owners.closed = true
		observers := make([]*Observer, 0, len(owners.members))
		for observer := range owners.members {
			observers = append(observers, observer)
		}
		owners.mu.Unlock()
		for _, observer := range observers {
			_ = observer.Close()
		}
		owners.owners.Wait()
		close(owners.done)
	})
	<-owners.done
}

func (client *Client) watchStatusView(ctx context.Context, scope protocol.PluginUiStatusScope) *Observer {
	observer := newObserver(ctx)
	scope = copyValue(scope)
	view := StatusView{Entries: []protocol.PluginUiStatusEntry{}, Failures: []protocol.PluginUiStatusFailure{}}
	observer.read = func() (any, error) {
		observer.mu.Lock()
		defer observer.mu.Unlock()
		return copyValue(view), nil
	}
	if !scope.Valid() {
		view.Error = observerFault(fault(protocol.ErrorCodeInvalidRequest, "status scope requires a worktree or session"))
		observer.notify()
		return observer
	}
	owners := client.pluginStatusObservers
	if owners == nil {
		view.Error = observerFault(fault(protocol.ErrorCodeUnavailable, "Client Status observation is unavailable"))
		observer.notify()
		return observer
	}
	client.lifecycle.Lock()
	lifetime := client.context
	client.lifecycle.Unlock()
	var stopClient func() bool
	if lifetime != nil {
		stopClient = context.AfterFunc(lifetime, observer.cancel)
	}
	observer.release = func() {
		if stopClient != nil {
			stopClient()
		}
		owners.remove(observer)
	}
	if !owners.add(observer) {
		view.Error = observerFault(fault(protocol.ErrorCodeUnavailable, "Client is closed"))
		observer.notify()
		return observer
	}
	if !observer.run(func() {
		defer owners.owners.Done()
		for observer.ctx.Err() == nil {
			stream, err := client.Subscribe(observer.ctx, protocol.Topic{Kind: "status", Data: scope})
			if err == nil {
				err = client.readPluginStatus(observer, stream, scope, &view)
				_ = stream.Close()
			}
			if observer.ctx.Err() != nil {
				return
			}
			observer.mu.Lock()
			view.Connected, view.Error = false, observerFault(err)
			observer.mu.Unlock()
			observer.notify()
			timer := time.NewTimer(500 * time.Millisecond)
			select {
			case <-observer.ctx.Done():
				timer.Stop()
				return
			case <-timer.C:
			}
		}
	}) {
		owners.owners.Done()
	}
	return observer
}

func (client *Client) readPluginStatus(observer *Observer, stream link.Subscription, scope protocol.PluginUiStatusScope, view *StatusView) error {
	for observer.ctx.Err() == nil {
		update, err := stream.Next(observer.ctx)
		if err != nil {
			return err
		}
		snapshot, err := payload[protocol.PluginUiStatusSnapshot](update.Data)
		if update.Kind != "status" || err != nil || snapshot.Node != client.Target() || !reflect.DeepEqual(snapshot.Scope, scope) {
			return fault(protocol.ErrorCodeWrongTarget, "status snapshot belongs to another scope")
		}
		snapshot = copyValue(snapshot)
		entries := []protocol.PluginUiStatusEntry{}
		for _, entry := range snapshot.Entries {
			if protocol.ValidPluginUiStatusState(entry.State, entry.Declaration) {
				entries = append(entries, entry)
			}
		}
		slices.SortFunc(entries, func(first, second protocol.PluginUiStatusEntry) int {
			return cmp.Or(cmp.Compare(first.Declaration.Order, second.Declaration.Order),
				cmp.Compare(first.Package.Name, second.Package.Name), cmp.Compare(first.Declaration.ID, second.Declaration.ID))
		})
		failures := snapshot.Failures
		if failures == nil {
			failures = []protocol.PluginUiStatusFailure{}
		}
		observer.mu.Lock()
		view.Entries, view.Failures, view.Connected, view.Error = entries, failures, true, nil
		observer.mu.Unlock()
		observer.notify()
	}
	return observer.ctx.Err()
}

func (client *Client) PrepareStatus(scope protocol.PluginUiStatusScope, entry protocol.PluginUiStatusEntry) (protocol.Request, error) {
	if scope.Session != nil && scope.Worktree == nil {
		return protocol.Request{}, fault(protocol.ErrorCodeInvalidRequest, "status action requires the session worktree")
	}
	if !scope.Valid() || !protocol.ValidPluginUiStatusState(entry.State, entry.Declaration) ||
		entry.Declaration.Kind != protocol.PluginUiKindButton || !entry.State.Visible || !entry.State.Enabled {
		return protocol.Request{}, fault(protocol.ErrorCodeInvalidRequest, "status action is unavailable")
	}
	if entry.Invoke == nil {
		return protocol.Request{}, fault(protocol.ErrorCodeInvalidRequest, "status action has no host handler")
	}
	scope = copyValue(scope)
	input, err := json.Marshal(map[string]string{"id": entry.Declaration.ID})
	if err != nil {
		return protocol.Request{}, err
	}
	request := client.Prepare(protocol.Command{Kind: "call_plugin", Data: protocol.CommandCallPluginPayload{Handler: *entry.Invoke, Input: input}})
	request.Plugin = &protocol.PluginContext{
		Surface: protocol.PluginDesktopSurface{Kind: "composer"}, Package: entry.Package,
		Worktree: scope.Worktree, Session: scope.Session,
	}
	return request, nil
}
