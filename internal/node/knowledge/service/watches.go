package service

import (
	"log/slog"
	"os"
	"path/filepath"
	"slices"
	"sync"
	"sync/atomic"

	"github.com/sailry/sailry-harness/internal/node/fswatch"
	"github.com/sailry/sailry-harness/internal/node/knowledge/source"
)

type rules map[string][]source.Watch

// watches follows the directories a pass scanned and requests another pass
// when an indexed or newly eligible entry changes. A missing or failed
// backend leaves the periodic rescan active.
type watches struct {
	refresh    func()
	backend    *fswatch.Watcher
	registered map[string]bool
	desired    rules
	incomplete bool
	changed    bool
	failed     atomic.Bool

	mu    sync.Mutex
	rules rules
}

func newWatches(refresh func()) *watches {
	watches := &watches{refresh: refresh, registered: map[string]bool{}, desired: rules{}, rules: rules{}}
	watches.start()
	return watches
}

func (watches *watches) start() {
	watches.close()
	backend, err := fswatch.New()
	if err != nil {
		return
	}
	watches.backend = backend
	go func() {
		for {
			select {
			case batch, open := <-backend.Events:
				if !open {
					return
				}
				if slices.ContainsFunc(batch, watches.relevant) {
					watches.refresh()
				}
			case _, open := <-backend.Errors:
				if !open {
					return
				}
				watches.failed.Store(true)
				watches.refresh()
			}
		}
	}()
}

func (watches *watches) close() {
	if watches.backend != nil {
		_ = watches.backend.Close()
		watches.backend = nil
	}
}

func (watches *watches) relevant(event fswatch.Event) bool {
	watches.mu.Lock()
	defer watches.mu.Unlock()
	if _, watched := watches.rules[event.Path]; watched {
		if event.Lost {
			// A removed or renamed directory invalidates its registration.
			watches.failed.Store(true)
		}
		return true
	}
	parent, found := watches.rules[filepath.Dir(event.Path)]
	if !found {
		return false
	}
	name := filepath.Base(event.Path)
	metadata, err := os.Lstat(event.Path)
	for _, rule := range parent {
		if err == nil && rule.Accepts(name, metadata.IsDir()) || err != nil && (rule.Accepts(name, false) || rule.Accepts(name, true)) {
			return true
		}
	}
	return false
}

func (watches *watches) begin() {
	watches.desired = rules{}
	if watches.failed.Swap(false) || watches.backend == nil {
		// A backend error invalidates every registration; rebuild instead
		// of treating retained path names as functioning watches.
		watches.start()
		watches.registered = map[string]bool{}
	}
}

func (watches *watches) add(base string, found []source.Watch) {
	for _, watch := range found {
		path := filepath.Join(base, filepath.FromSlash(watch.Relative))
		watches.desired[path] = append(watches.desired[path], watch)
		// Register before the new directory is extracted or encoded. Earlier
		// roots stay active until a complete pass finishes.
		watches.mu.Lock()
		watches.rules[path] = watches.desired[path]
		watches.mu.Unlock()
		if watches.registered[path] || watches.backend == nil {
			continue
		}
		if err := watches.backend.Dir(base, path); err != nil {
			watches.failed.Store(true)
			slog.Warn("knowledge watch unavailable; periodic rescan remains active", "error", err)
			continue
		}
		watches.registered[path] = true
		watches.changed = true
	}
}

func (watches *watches) finish(complete bool) {
	watches.incomplete = !complete
	if complete {
		for path := range watches.registered {
			if _, wanted := watches.desired[path]; wanted {
				continue
			}
			// A removed directory already dropped its own registration.
			if watches.backend != nil {
				watches.backend.Drop(path)
			}
			delete(watches.registered, path)
			watches.changed = true
		}
		watches.mu.Lock()
		watches.rules = watches.desired
		watches.desired = rules{}
		watches.mu.Unlock()
	}
	// Recheck after the watched topology settles, including writes made
	// while it was being registered.
	changed := watches.changed
	watches.changed = false
	if changed && watches.backend != nil && !watches.failed.Load() {
		watches.refresh()
	}
}

func (watches *watches) needsPoll() bool {
	return watches.backend == nil || watches.incomplete || watches.failed.Load()
}
