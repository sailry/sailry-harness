package client

import (
	"context"
	"encoding/json"
	"errors"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Observer struct {
	ctx     context.Context
	cancel  context.CancelFunc
	mu      sync.Mutex
	next    chan struct{}
	closed  bool
	signal  chan struct{}
	read    func() (any, error)
	control func(context.Context, string, json.RawMessage) error
	release func()
	workers sync.WaitGroup
}

func newObserver(ctx context.Context) *Observer {
	lifetime, cancel := context.WithCancel(ctx)
	return &Observer{ctx: lifetime, cancel: cancel, next: make(chan struct{}, 1), signal: make(chan struct{}, 1)}
}

func (observer *Observer) notify() {
	select {
	case observer.signal <- struct{}{}:
	default:
	}
}

func (observer *Observer) run(work func()) bool {
	observer.mu.Lock()
	defer observer.mu.Unlock()
	if observer.closed {
		return false
	}
	observer.workers.Add(1)
	go func() {
		defer observer.workers.Done()
		work()
	}()
	return true
}

func (observer *Observer) Next(ctx context.Context) (json.RawMessage, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-observer.ctx.Done():
		return nil, observer.ctx.Err()
	case observer.next <- struct{}{}:
	}
	defer func() { <-observer.next }()
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-observer.ctx.Done():
		return nil, observer.ctx.Err()
	case <-observer.signal:
	}
	if err := observer.ctx.Err(); err != nil {
		return nil, err
	}
	value, err := observer.read()
	if err != nil {
		return nil, err
	}
	encoded, err := json.Marshal(value)
	return json.RawMessage(encoded), err
}

func (observer *Observer) Control(ctx context.Context, method string, argument json.RawMessage) error {
	if err := observer.ctx.Err(); err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if observer.control == nil {
		return fault(protocol.ErrorCodeInvalidRequest, "observer control is unavailable")
	}
	return observer.control(ctx, method, append(json.RawMessage{}, argument...))
}

func (observer *Observer) Close() error {
	observer.mu.Lock()
	if observer.closed {
		observer.mu.Unlock()
		return nil
	}
	observer.closed = true
	observer.cancel()
	release := observer.release
	observer.mu.Unlock()
	if release != nil {
		release()
	}
	observer.workers.Wait()
	return nil
}

func observerFault(err error) *protocol.Fault {
	if err == nil {
		return nil
	}
	var direct *protocol.Fault
	if errors.As(err, &direct) {
		return direct
	}
	var wrapped *link.FaultError
	if errors.As(err, &wrapped) {
		return &wrapped.Fault
	}
	var value protocol.Fault
	if errors.As(err, &value) {
		return &value
	}
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: err.Error()}
}

func (client *Client) Watch(ctx context.Context, kind string, argument json.RawMessage) (*Observer, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	switch kind {
	case "watch":
		return client.watchView(ctx), nil
	case "watch_commands", "watch_conversation":
		var session protocol.SessionID
		if err := json.Unmarshal(argument, &session); err != nil {
			return nil, fault(protocol.ErrorCodeInvalidRequest, "invalid session identifier")
		}
		if kind == "watch_commands" {
			return client.watchCommandView(ctx, session)
		}
		return client.watchConversationView(ctx, session)
	case "watch_usage":
		var query protocol.UsageQuery
		if err := json.Unmarshal(argument, &query); err != nil {
			return nil, fault(protocol.ErrorCodeInvalidRequest, "invalid usage query")
		}
		return client.watchUsageView(ctx, query), nil
	case "watch_status":
		var scope protocol.PluginUiStatusScope
		if json.Unmarshal(argument, &scope) != nil || scope.Worktree == nil && scope.Session == nil {
			return nil, fault(protocol.ErrorCodeInvalidRequest, "status scope requires a worktree or session")
		}
		return client.watchStatusView(ctx, scope), nil
	case "watch_terminal":
		var terminal protocol.TerminalID
		if err := json.Unmarshal(argument, &terminal); err != nil {
			return nil, fault(protocol.ErrorCodeInvalidRequest, "invalid terminal identifier")
		}
		return unavailableObserver(ctx, map[string]any{
			"snapshot": nil, "connected": false, "recovering": false,
		}, "terminal observation is unavailable"), nil
	case "watch_login", "watch_connector_login":
		if !json.Valid(argument) {
			return nil, fault(protocol.ErrorCodeInvalidRequest, "invalid login attempt")
		}
		return unavailableObserver(ctx, map[string]any{
			"update": nil, "connected": false,
		}, "authorization observation is unavailable"), nil
	case "watch_connections":
		return nil, fault(protocol.ErrorCodeUnavailable, "plugin connection observation is unavailable")
	}
	return nil, fault(protocol.ErrorCodeInvalidRequest, "unknown Client observation")
}

func unavailableObserver(ctx context.Context, value map[string]any, message string) *Observer {
	observer := newObserver(ctx)
	value["error"] = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}
	observer.read = func() (any, error) { return value, nil }
	observer.notify()
	return observer
}

func (client *Client) startObserver(ctx context.Context, fresh bool) error {
	if fresh {
		client.lifecycle.Lock()
		started := client.context != nil && !client.closed
		client.lifecycle.Unlock()
		if started {
			return client.Refresh(ctx)
		}
	}
	lifetime, cancel := context.WithCancel(context.Background())
	stop := context.AfterFunc(ctx, cancel)
	err := client.Start(lifetime)
	if !stop() || err != nil {
		cancel()
	}
	return err
}

func (client *Client) watchView(ctx context.Context) *Observer {
	observer := newObserver(ctx)
	var initial error
	observer.read = func() (any, error) {
		client.mu.RLock()
		snapshot := client.project.Snapshot()
		connected, failure := client.connected, client.err
		client.mu.RUnlock()
		observer.mu.Lock()
		startup := initial
		observer.mu.Unlock()
		telemetry := client.Connection()
		if failure == nil {
			failure = startup
		}
		if !connected {
			telemetry = link.ConnectionInfo{}
		}
		targets := map[string]protocol.SessionID{}
		if snapshot != nil {
			targets = childTargets(*snapshot)
		}
		return map[string]any{
			"snapshot": snapshot, "connected": connected, "connection": telemetry,
			"error": observerFault(failure), "notifications": client.Notices(),
			"child_targets": targets,
		}, nil
	}
	observer.release = client.OnChange(observer.notify)
	observer.run(func() {
		err := client.startObserver(observer.ctx, true)
		observer.mu.Lock()
		initial = err
		observer.mu.Unlock()
		observer.notify()
	})
	return observer
}

func (client *Client) watchCommandView(ctx context.Context, session protocol.SessionID) (*Observer, error) {
	observer := newObserver(ctx)
	release, err := client.ObserveCommands(observer.ctx, session, observer.notify)
	if err != nil {
		_ = observer.Close()
		return nil, err
	}
	observer.release = release
	observer.read = func() (any, error) {
		state, _ := client.Commands(session)
		items := state.Items
		if items == nil {
			items = []protocol.ProcessInfo{}
		}
		return map[string]any{
			"items": items, "output": nil, "connected": state.Connected, "error": observerFault(state.Err),
		}, nil
	}
	observer.notify()
	return observer, nil
}

func (client *Client) PrepareStatusJSON(scopeData, entryData json.RawMessage) (protocol.Request, error) {
	var scope protocol.PluginUiStatusScope
	var entry protocol.PluginUiStatusEntry
	if json.Unmarshal(scopeData, &scope) != nil || json.Unmarshal(entryData, &entry) != nil {
		return protocol.Request{}, fault(protocol.ErrorCodeInvalidRequest, "invalid status action capture")
	}
	return client.PrepareStatus(scope, entry)
}

func childTarget(child protocol.ConversationChild) string {
	encoded, _ := json.Marshal([]any{child.Run.Session, child.Run.Worktree, child.Origin.Session, child.Origin.Role})
	return string(encoded)
}

func childTargets(snapshot protocol.Snapshot) map[string]protocol.SessionID {
	result := map[string]protocol.SessionID{}
	for _, session := range snapshot.Sessions {
		if session.Delegation == nil || *session.Delegation == nil {
			continue
		}
		origin := **session.Delegation
		child := protocol.ConversationChild{Run: protocol.ConversationRun{Session: session.ID, Worktree: session.Worktree}, Origin: origin}
		if _, valid := ConversationChildSession(snapshot, origin.Session, child); valid {
			result[childTarget(child)] = session.ID
		}
	}
	return result
}
