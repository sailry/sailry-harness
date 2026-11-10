package node

import (
	"context"
	"database/sql"
	"slices"
	"sync"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	pluginStatusDebounce    = 750 * time.Millisecond
	pluginStatusMaximumWait = time.Second
	pluginStatusFailureWait = 30 * time.Second
)

type pluginStatusRunner struct {
	service      *pluginStatusService
	watcher      *pluginStatusWatcher
	effective    protocol.PluginUiStatusScope
	packages     map[string]*pluginStatusPackage
	generation   uint64
	events       link.Subscription
	eventUpdates chan pluginStatusUpdate
	repository   *pluginStatusRepository
	repositoryID uint64
	repositories chan pluginStatusRepositoryUpdate
	completed    chan pluginStatusCompleted
	workers      sync.WaitGroup
}

type pluginStatusPackage struct {
	info       protocol.PluginInfo
	generation uint64
	entries    []protocol.PluginUiStatusEntry
	failure    *protocol.Fault
	reported   bool
	retryAfter time.Time
	dirty      *pluginStatusDirty
	running    bool
	ctx        context.Context
	cancel     context.CancelFunc
}

type pluginStatusDirty struct {
	first  time.Time
	latest time.Time
	force  bool
}

type pluginStatusUpdate struct {
	update protocol.Update
	err    error
}

type pluginStatusCompleted struct {
	name       string
	generation uint64
	entries    []protocol.PluginUiStatusEntry
	err        error
}

type pluginStatusRepository struct {
	root   string
	stream link.Subscription
	cancel context.CancelFunc
	done   chan struct{}
}

type pluginStatusRepositoryUpdate struct {
	generation uint64
	err        error
}

func (service *pluginStatusService) run(watcher *pluginStatusWatcher, events link.Subscription) {
	defer service.owners.Done()
	defer close(watcher.done)
	runner := &pluginStatusRunner{
		service: service, watcher: watcher, effective: watcher.scope, events: events,
		packages: map[string]*pluginStatusPackage{}, eventUpdates: make(chan pluginStatusUpdate, 1),
		repositories: make(chan pluginStatusRepositoryUpdate, 1), completed: make(chan pluginStatusCompleted, 4),
	}
	defer runner.close()
	runner.workers.Add(1)
	go func() {
		defer runner.workers.Done()
		for watcher.ctx.Err() == nil {
			update, err := events.Next(watcher.ctx)
			select {
			case <-watcher.ctx.Done():
				return
			case runner.eventUpdates <- pluginStatusUpdate{update: update, err: err}:
			}
			if err != nil {
				return
			}
		}
	}()
	if err := runner.refresh(""); err != nil {
		runner.finish(err)
		return
	}
	for watcher.ctx.Err() == nil {
		runner.launch()
		var timer clockwork.Timer
		var fired <-chan time.Time
		if deadline, pending := runner.deadline(); pending {
			timer = service.clock.NewTimer(deadline.Sub(service.clock.Now()))
			fired = timer.Chan()
		}
		var failure error
		select {
		case <-watcher.ctx.Done():
		case update := <-runner.eventUpdates:
			if update.err != nil {
				failure = update.err
			} else {
				failure = runner.update(update.update)
			}
		case <-watcher.wake:
			for _, name := range watcher.takeSignals() {
				runner.mark(name, true)
			}
		case update := <-runner.repositories:
			if runner.repository != nil && update.generation == runner.repositoryID {
				if update.err != nil {
					failure = update.err
				} else {
					runner.changed(protocol.PluginHostChangeRepository)
				}
			}
		case completed := <-runner.completed:
			runner.complete(completed)
		case <-fired:
		}
		if timer != nil {
			timer.Stop()
		}
		if failure != nil {
			runner.finish(failure)
			return
		}
	}
}

func (runner *pluginStatusRunner) close() {
	runner.watcher.cancel()
	_ = runner.events.Close()
	runner.closeRepository()
	for _, entry := range runner.packages {
		entry.cancel()
	}
	runner.workers.Wait()
}

func (runner *pluginStatusRunner) refresh(changed string) error {
	effective, err := runner.service.node.resolvePluginStatusScope(runner.watcher.ctx, runner.watcher.scope)
	if err != nil {
		return err
	}
	var selected []protocol.PluginInfo
	err = runner.service.node.profile.Store.Node.Read(runner.watcher.ctx, func(db *sql.Conn) error {
		var err error
		selected, err = store.ReadPluginStatusPackages(runner.watcher.ctx, db, effective)
		return err
	})
	if err != nil {
		return err
	}
	if pluginStatusKey(effective) != pluginStatusKey(runner.effective) {
		for _, entry := range runner.packages {
			entry.cancel()
		}
		clear(runner.packages)
		runner.closeRepository()
		runner.effective = effective
	}
	for name, entry := range runner.packages {
		if !slices.ContainsFunc(selected, func(info protocol.PluginInfo) bool { return info.Summary.Name == name }) {
			entry.cancel()
			delete(runner.packages, name)
		}
	}
	for _, info := range selected {
		name := info.Summary.Name
		if current := runner.packages[name]; current != nil && plugins.PackageReference(current.info) == plugins.PackageReference(info) {
			current.info = info
			if changed == name {
				current.mark(runner.service.clock.Now(), true)
			}
			continue
		}
		if previous := runner.packages[name]; previous != nil {
			previous.cancel()
		}
		runner.generation++
		lifetime, cancel := context.WithCancel(runner.watcher.ctx)
		now := runner.service.clock.Now()
		runner.packages[name] = &pluginStatusPackage{
			info: info, generation: runner.generation, entries: store.PluginStatusEntries(info, nil),
			dirty: &pluginStatusDirty{first: now, latest: now, force: true}, ctx: lifetime, cancel: cancel,
		}
	}
	runner.publish(nil)
	return runner.watchRepository()
}

func (runner *pluginStatusRunner) update(update protocol.Update) error {
	switch update.Kind {
	case "snapshot":
		return nil
	case "reset_required":
		if err := runner.refresh(""); err != nil {
			return err
		}
		runner.mark("", false)
		return nil
	case "event":
		envelope, valid := dispatchPayload[protocol.EventEnvelope](update.Data)
		if !valid {
			return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid Status source event"}
		}
		return runner.event(envelope.Event)
	default:
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid Status source update"}
	}
}

func (runner *pluginStatusRunner) event(event protocol.Event) error {
	switch event.Kind {
	case "worktree_removed":
		value, valid := dispatchPayload[protocol.EventWorktreeRemovedPayload](event.Data)
		if valid && runner.effective.Worktree != nil && *runner.effective.Worktree == value.ID {
			return &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "status worktree was removed"}
		}
	case "sessions_removed":
		values, valid := dispatchPayload[[]protocol.SessionID](event.Data)
		if valid && runner.watcher.scope.Session != nil && slices.Contains(values, *runner.watcher.scope.Session) {
			return &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "status session was removed"}
		}
	case "plugin_changed":
		value, valid := dispatchPayload[protocol.PluginSummary](event.Data)
		if valid {
			return runner.refresh(value.Name)
		}
	case "plugin_removed":
		value, valid := dispatchPayload[protocol.EventPluginRemovedPayload](event.Data)
		if valid {
			return runner.refresh(value.Name)
		}
	case "plugin_values_changed":
		value, valid := dispatchPayload[protocol.EventPluginValuesChangedPayload](event.Data)
		if entry := runner.packages[value.Name]; valid && entry != nil && entry.changes(protocol.PluginHostChangePackage) {
			entry.mark(runner.service.clock.Now(), false)
		}
	case "session_changed":
		value, valid := dispatchPayload[protocol.Session](event.Data)
		if valid && runner.watcher.scope.Session != nil && *runner.watcher.scope.Session == value.ID {
			if err := runner.refresh(""); err != nil {
				return err
			}
			runner.changed(protocol.PluginHostChangeConversation)
		}
	case "conversation_changed":
		value, valid := dispatchPayload[protocol.EventConversationChangedPayload](event.Data)
		if valid && runner.watcher.scope.Session != nil && *runner.watcher.scope.Session == value.Session {
			runner.changed(protocol.PluginHostChangeConversation)
		}
	case "tools_completed":
		value, valid := dispatchPayload[protocol.EventToolsCompletedPayload](event.Data)
		if valid && runner.watcher.scope.Session != nil && *runner.watcher.scope.Session == value.Session {
			runner.changed(protocol.PluginHostChangeConversation)
		}
	case "turn_queued":
		value, valid := dispatchPayload[protocol.EventTurnQueuedPayload](event.Data)
		if valid && runner.watcher.scope.Session != nil && *runner.watcher.scope.Session == value.Turn.Session {
			runner.changed(protocol.PluginHostChangeConversation)
		}
	}
	return nil
}

func (runner *pluginStatusRunner) mark(name string, force bool) {
	now := runner.service.clock.Now()
	for key, entry := range runner.packages {
		if name == "" || key == name {
			entry.mark(now, force)
		}
	}
}

func (runner *pluginStatusRunner) changed(change protocol.PluginHostChange) {
	now := runner.service.clock.Now()
	for _, entry := range runner.packages {
		if entry.changes(change) {
			entry.mark(now, false)
		}
	}
}

func (runner *pluginStatusRunner) deadline() (time.Time, bool) {
	var next time.Time
	found := false
	for _, entry := range runner.packages {
		if deadline, pending := entry.deadline(); pending && (!found || deadline.Before(next)) {
			next, found = deadline, true
		}
	}
	return next, found
}

func (runner *pluginStatusRunner) launch() {
	now := runner.service.clock.Now()
	for name, entry := range runner.packages {
		deadline, pending := entry.deadline()
		if !pending || deadline.After(now) {
			continue
		}
		entry.running, entry.dirty = true, nil
		generation, info, lifetime, scope := entry.generation, entry.info, entry.ctx, runner.effective
		runner.workers.Add(1)
		go func() {
			defer runner.workers.Done()
			entries, err := runner.service.read(lifetime, scope, info)
			select {
			case <-runner.watcher.ctx.Done():
			case runner.completed <- pluginStatusCompleted{name: name, generation: generation, entries: entries, err: err}:
			}
		}()
	}
}

func (runner *pluginStatusRunner) complete(completed pluginStatusCompleted) {
	entry := runner.packages[completed.name]
	if entry == nil || entry.generation != completed.generation {
		return
	}
	if report := entry.complete(completed.entries, completed.err, runner.service.clock.Now()); report != nil {
		runner.service.node.reportPluginStatusFailure(runner.watcher.ctx, report, entry.info)
	}
	runner.publish(nil)
}

func (runner *pluginStatusRunner) publish(terminal error) {
	snapshot := protocol.PluginUiStatusSnapshot{
		Node: runner.service.node.ID(), Scope: runner.watcher.scope,
		Entries: []protocol.PluginUiStatusEntry{}, Failures: []protocol.PluginUiStatusFailure{},
	}
	names := make([]string, 0, len(runner.packages))
	for name := range runner.packages {
		names = append(names, name)
	}
	slices.Sort(names)
	for _, name := range names {
		entry := runner.packages[name]
		snapshot.Entries = append(snapshot.Entries, entry.entries...)
		if entry.failure != nil {
			snapshot.Failures = append(snapshot.Failures, protocol.PluginUiStatusFailure{Package: plugins.PackageReference(entry.info), Fault: *entry.failure})
		}
	}
	runner.watcher.publish(pluginStatusFrame{snapshot: snapshot, terminal: pluginStatusPublicFault(terminal)})
}

func (runner *pluginStatusRunner) finish(err error) {
	failure := pluginStatusPublicFault(err)
	if failure.Code == protocol.ErrorCodeNotFound || failure.Code == protocol.ErrorCodeWrongTarget {
		for _, entry := range runner.packages {
			entry.cancel()
		}
		clear(runner.packages)
	}
	runner.closeRepository()
	runner.publish(err)
}

func (entry *pluginStatusPackage) changes(change protocol.PluginHostChange) bool {
	return entry.info.Extension != nil && entry.info.Extension.Host != nil && entry.info.Extension.Host.Status != nil &&
		slices.Contains(entry.info.Extension.Host.Status.Changes, change)
}

func (entry *pluginStatusPackage) mark(now time.Time, force bool) {
	if entry.dirty == nil {
		entry.dirty = &pluginStatusDirty{first: now, latest: now, force: force}
	}
	entry.dirty.latest = now
	entry.dirty.force = entry.dirty.force || force
}

func (entry *pluginStatusPackage) deadline() (time.Time, bool) {
	if entry.running || entry.dirty == nil {
		return time.Time{}, false
	}
	return entry.dirty.due(entry.retryAfter), true
}

func (entry *pluginStatusPackage) complete(entries []protocol.PluginUiStatusEntry, err error, now time.Time) error {
	entry.running = false
	if err == nil {
		entry.entries, entry.failure, entry.reported, entry.retryAfter = entries, nil, false, time.Time{}
		return nil
	}
	failure := pluginStatusPublicFault(err)
	retry := failure.Code == protocol.ErrorCodeBusy && entry.failure == nil
	entry.failure, entry.retryAfter = failure, now.Add(pluginStatusFailureWait)
	if retry {
		entry.dirty = &pluginStatusDirty{first: now, latest: now}
		return nil
	}
	if entry.reported {
		return nil
	}
	entry.reported = true
	return err
}

func (dirty pluginStatusDirty) due(retryAfter time.Time) time.Time {
	if dirty.force {
		return dirty.latest
	}
	due := dirty.latest.Add(pluginStatusDebounce)
	if maximum := dirty.first.Add(pluginStatusMaximumWait); maximum.Before(due) {
		due = maximum
	}
	if retryAfter.After(due) {
		due = retryAfter
	}
	return due
}
