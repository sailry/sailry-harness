package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"
	"sync"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginStatusService struct {
	node     *Node
	mu       sync.Mutex
	watchers map[pluginStatusScopeKey]*pluginStatusWatcher
	permits  chan struct{}
	clock    clockwork.Clock
	closed   bool
	owners   sync.WaitGroup
}

type pluginStatusScopeKey struct {
	worktree    protocol.WorktreeID
	session     protocol.SessionID
	hasWorktree bool
	hasSession  bool
}

type pluginStatusWatcher struct {
	service    *pluginStatusService
	key        pluginStatusScopeKey
	scope      protocol.PluginUiStatusScope
	ctx        context.Context
	cancel     context.CancelFunc
	done       chan struct{}
	references uint64
	mu         sync.Mutex
	frame      *pluginStatusFrame
	revision   uint64
	changed    chan struct{}
	signals    []string
	wake       chan struct{}
}

type pluginStatusFrame struct {
	snapshot protocol.PluginUiStatusSnapshot
	terminal *protocol.Fault
}

type pluginStatusFeed struct {
	watcher *pluginStatusWatcher
	closed  chan struct{}
	once    sync.Once
	next    sync.Mutex
	current uint64
	failure *protocol.Fault
}

func (node *Node) initPluginStatus() {
	clock := clockwork.NewRealClock()
	if node.dispatch != nil && node.dispatch.clock != nil {
		clock = node.dispatch.clock
	}
	node.pluginStatus = &pluginStatusService{
		node: node, clock: clock, permits: make(chan struct{}, 4),
		watchers: map[pluginStatusScopeKey]*pluginStatusWatcher{},
	}
}

func (node *Node) closePluginStatus() {
	service := node.pluginStatus
	if service == nil {
		return
	}
	service.mu.Lock()
	service.closed = true
	for _, watcher := range service.watchers {
		watcher.cancel()
	}
	service.mu.Unlock()
	service.owners.Wait()
}

func (node *Node) subscribePluginStatus(ctx context.Context, _ protocol.NodeID, topic protocol.Topic) (link.Subscription, error) {
	scope, valid := dispatchPayload[protocol.PluginUiStatusScope](topic.Data)
	if !valid {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "status requires a worktree or session"}
	}
	scope = pluginStatusScopeCopy(scope)
	if err := node.begin(); err != nil {
		return nil, err
	}
	defer node.work.Done()
	service := node.pluginStatus
	if service == nil {
		return nil, unavailable()
	}
	events, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		return nil, err
	}
	if _, err := node.resolvePluginStatusScope(ctx, scope); err != nil {
		_ = events.Close()
		return nil, nodeFault(err)
	}
	key := pluginStatusKey(scope)
	service.mu.Lock()
	if service.closed || node.ctx.Err() != nil {
		service.mu.Unlock()
		_ = events.Close()
		return nil, unavailable()
	}
	watcher := service.watchers[key]
	if watcher != nil && watcher.active() && watcher.signal("") {
		watcher.references++
		service.mu.Unlock()
		_ = events.Close()
		return &pluginStatusFeed{watcher: watcher, closed: make(chan struct{})}, nil
	}
	lifetime, cancel := context.WithCancel(node.ctx)
	watcher = &pluginStatusWatcher{
		service: service, key: key, scope: scope, ctx: lifetime, cancel: cancel,
		done: make(chan struct{}), references: 1, changed: make(chan struct{}), wake: make(chan struct{}, 1),
	}
	service.watchers[key] = watcher
	service.owners.Add(1)
	service.mu.Unlock()
	go service.run(watcher, events)
	return &pluginStatusFeed{watcher: watcher, closed: make(chan struct{})}, nil
}

func (node *Node) invalidatePluginStatus(name string) {
	service := node.pluginStatus
	if service == nil {
		return
	}
	service.mu.Lock()
	defer service.mu.Unlock()
	if !service.closed {
		for _, watcher := range service.watchers {
			watcher.signal(name)
		}
	}
}

func (node *Node) resolvePluginStatusScope(ctx context.Context, scope protocol.PluginUiStatusScope) (protocol.PluginUiStatusScope, error) {
	var effective protocol.PluginUiStatusScope
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		effective, err = store.ResolvePluginStatusScope(ctx, db, scope)
		return err
	})
	if err == nil && effective.Worktree != nil {
		_, err = node.workspaceRoot(ctx, *effective.Worktree, false)
	}
	return effective, err
}

func (watcher *pluginStatusWatcher) active() bool {
	watcher.mu.Lock()
	defer watcher.mu.Unlock()
	return watcher.ctx.Err() == nil && (watcher.frame == nil || watcher.frame.terminal == nil)
}

func (watcher *pluginStatusWatcher) signal(name string) bool {
	watcher.mu.Lock()
	defer watcher.mu.Unlock()
	if watcher.ctx.Err() != nil {
		return false
	}
	watcher.signals = append(watcher.signals, name)
	select {
	case watcher.wake <- struct{}{}:
	default:
	}
	return true
}

func (watcher *pluginStatusWatcher) takeSignals() []string {
	watcher.mu.Lock()
	defer watcher.mu.Unlock()
	signals := watcher.signals
	watcher.signals = nil
	return signals
}

func (watcher *pluginStatusWatcher) publish(frame pluginStatusFrame) {
	watcher.mu.Lock()
	defer watcher.mu.Unlock()
	if reflect.DeepEqual(watcher.frame, &frame) {
		return
	}
	watcher.frame = &frame
	watcher.revision++
	close(watcher.changed)
	watcher.changed = make(chan struct{})
}

func (feed *pluginStatusFeed) Close() error {
	feed.once.Do(func() {
		close(feed.closed)
		service := feed.watcher.service
		service.mu.Lock()
		feed.watcher.references--
		last := feed.watcher.references == 0
		if last {
			if service.watchers[feed.watcher.key] == feed.watcher {
				delete(service.watchers, feed.watcher.key)
			}
			feed.watcher.cancel()
		}
		service.mu.Unlock()
		if last {
			<-feed.watcher.done
		}
	})
	return nil
}

func (feed *pluginStatusFeed) Next(ctx context.Context) (protocol.Update, error) {
	feed.next.Lock()
	defer feed.next.Unlock()
	if feed.failure != nil {
		return protocol.Update{}, feed.failure
	}
	for {
		feed.watcher.mu.Lock()
		frame, revision, changed := feed.watcher.frame, feed.watcher.revision, feed.watcher.changed
		feed.watcher.mu.Unlock()
		if frame != nil && revision != feed.current {
			body, err := json.Marshal(frame.snapshot)
			if err != nil {
				return protocol.Update{}, nodeFault(err)
			}
			var snapshot protocol.PluginUiStatusSnapshot
			if err := json.Unmarshal(body, &snapshot); err != nil {
				return protocol.Update{}, nodeFault(err)
			}
			feed.current, feed.failure = revision, frame.terminal
			return protocol.Update{Kind: "status", Data: snapshot}, nil
		}
		select {
		case <-ctx.Done():
			return protocol.Update{}, ctx.Err()
		case <-feed.closed:
			return protocol.Update{}, unavailable()
		case <-feed.watcher.service.node.closed:
			return protocol.Update{}, unavailable()
		case <-changed:
		}
	}
}

func pluginStatusKey(scope protocol.PluginUiStatusScope) pluginStatusScopeKey {
	key := pluginStatusScopeKey{hasWorktree: scope.Worktree != nil, hasSession: scope.Session != nil}
	if scope.Worktree != nil {
		key.worktree = *scope.Worktree
	}
	if scope.Session != nil {
		key.session = *scope.Session
	}
	return key
}

func pluginStatusScopeCopy(scope protocol.PluginUiStatusScope) protocol.PluginUiStatusScope {
	if scope.Worktree != nil {
		value := *scope.Worktree
		scope.Worktree = &value
	}
	if scope.Session != nil {
		value := *scope.Session
		scope.Session = &value
	}
	return scope
}
