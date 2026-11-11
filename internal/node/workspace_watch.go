package node

import (
	"context"
	"errors"
	"path/filepath"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/fswatch"
	workspacegit "github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// workspaceWatches owns one recursive watch per worktree root, shared by the
// subscriptions of that root and closed with the last of them.
type workspaceWatches struct {
	mu     sync.Mutex
	roots  map[string]*workspaceWatch
	closed bool
	git    *workspacegit.Service
	after  func(time.Duration) <-chan time.Time
}
type workspaceWatch struct {
	owner  *workspaceWatches
	root   string
	native *fswatch.Watcher
	// ready closes when the watch was opened or failed to open; started,
	// guarded by the owner, tells which.
	ready      chan struct{}
	started    bool
	done       chan struct{}
	mu         sync.Mutex
	filter     *workspacegit.WatchFilter
	references uint64
	failure    error
	observers  map[*workspaceEvents]struct{}
}
type workspaceEvents struct {
	active     *workspaceWatch
	node       protocol.NodeID
	worktree   protocol.WorktreeID
	repository bool
	// paths, when set, receives every batch of changed absolute paths
	// before the debounced update.
	paths   func([]string)
	wake    chan struct{}
	closed  chan struct{}
	once    sync.Once
	next    sync.Mutex
	initial bool
}

func newWorkspaceWatches(git *workspacegit.Service) *workspaceWatches {
	return &workspaceWatches{roots: map[string]*workspaceWatch{}, git: git, after: time.After}
}
func watchUnavailable() error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "file watch is unavailable"}
}

func (w *workspaceWatches) subscribe(ctx context.Context, root string, node protocol.NodeID, worktree protocol.WorktreeID, repository bool) (link.Subscription, error) {
	return w.observe(ctx, root, node, worktree, repository, nil)
}

// observe joins the watch of root, opening it for the first observer. The
// watch is opened outside the owner's lock, so other roots never wait for it.
func (w *workspaceWatches) observe(ctx context.Context, root string, node protocol.NodeID, worktree protocol.WorktreeID, repository bool, paths func([]string)) (link.Subscription, error) {
	for {
		w.mu.Lock()
		if w.closed || ctx.Err() != nil {
			w.mu.Unlock()
			return nil, watchUnavailable()
		}
		active := w.roots[root]
		if active == nil {
			if len(w.roots) >= 32 {
				w.mu.Unlock()
				return nil, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "file watch capacity exhausted"}
			}
			active = &workspaceWatch{owner: w, root: root, ready: make(chan struct{}), done: make(chan struct{}), observers: map[*workspaceEvents]struct{}{}}
			w.roots[root] = active
			w.mu.Unlock()
			err := active.open()
			w.mu.Lock()
			// A removal or shutdown during the open took the root away.
			current := w.roots[root] == active
			if err != nil || !current {
				if current {
					delete(w.roots, root)
				}
				close(active.ready)
				w.mu.Unlock()
				if active.native != nil {
					_ = active.native.Close()
				}
				if err == nil {
					err = watchUnavailable()
				}
				return nil, err
			}
			active.started = true
			close(active.ready)
			w.git.Invalidate(root)
			go active.run()
		} else if !active.started {
			w.mu.Unlock()
			select {
			case <-ctx.Done():
				return nil, watchUnavailable()
			case <-active.ready:
			}
			continue
		}
		observer := &workspaceEvents{active: active, node: node, worktree: worktree, repository: repository, paths: paths, wake: make(chan struct{}, 1), closed: make(chan struct{}), initial: true}
		active.mu.Lock()
		active.observers[observer] = struct{}{}
		active.mu.Unlock()
		w.mu.Unlock()
		return observer, nil
	}
}

// open watches the worktree and the Git directories outside it. Git's own
// directory inside the worktree is part of the tree.
func (w *workspaceWatch) open() error {
	filter, err := workspacegit.NewWatchFilter(w.root)
	if err != nil {
		return err
	}
	w.filter = filter
	watched, err := workspacegit.WatchRoots(w.root)
	if err != nil {
		return watchUnavailable()
	}
	if w.native, err = fswatch.New(); err != nil {
		return watchUnavailable()
	}
	trees := []string{w.root}
	for _, path := range watched {
		rel, e := filepath.Rel(w.root, path)
		if e != nil || rel == ".." || len(rel) > 3 && rel[:3] == ".."+string(filepath.Separator) {
			trees = append(trees, path)
		}
	}
	for _, tree := range trees {
		if err := w.native.Tree(tree); err != nil {
			return watchUnavailable()
		}
	}
	return nil
}
func (w *workspaceWatch) run() {
	defer close(w.done)
	for {
		select {
		case batch, ok := <-w.native.Events:
			if !ok {
				return
			}
			paths := make([]string, len(batch))
			for index, event := range batch {
				paths[index] = event.Path
			}
			w.notify(paths, nil, false)
		case _, ok := <-w.native.Errors:
			if !ok {
				return
			}
			w.notify(nil, watchUnavailable(), false)
			return
		}
	}
}

// stop ends a started watch and waits for its reader.
func (w *workspaceWatch) stop() error {
	err := w.native.Close()
	<-w.done
	return err
}
func (w *workspaceWatch) notify(paths []string, failure error, managed bool) {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.failure != nil {
		return
	}
	if failure != nil {
		w.failure = failure
	}
	references := managed || failure != nil || w.filter.ReferencesChanged(paths)
	relevant := managed || failure != nil || w.filter.Changed(paths)
	if references {
		w.references++
	}
	if relevant {
		w.owner.git.Invalidate(w.root)
	}
	for observer := range w.observers {
		if observer.paths != nil && len(paths) != 0 {
			observer.paths(paths)
		}
		if observer.repository && !relevant {
			continue
		}
		select {
		case observer.wake <- struct{}{}:
		default:
		}
	}
}
func (w *workspaceWatches) changed(root string, paths []string) {
	w.mu.Lock()
	active := w.roots[root]
	closed := w.closed
	w.mu.Unlock()
	if closed {
		return
	}
	if active != nil {
		active.notify(paths, nil, len(paths) == 0)
	} else {
		w.git.Invalidate(root)
	}
}
func (w *workspaceWatches) remove(root string) {
	w.mu.Lock()
	active := w.roots[root]
	delete(w.roots, root)
	// An opening watch sees the root gone and closes itself.
	started := active != nil && active.started
	w.mu.Unlock()
	if started {
		active.notify(nil, watchUnavailable(), false)
		_ = active.stop()
	}
}
func (w *workspaceWatches) close() error {
	w.mu.Lock()
	w.closed = true
	roots := w.roots
	w.roots = map[string]*workspaceWatch{}
	var started []*workspaceWatch
	for _, active := range roots {
		if active.started {
			started = append(started, active)
		}
	}
	w.mu.Unlock()
	var result error
	for _, active := range started {
		active.notify(nil, watchUnavailable(), false)
		result = errors.Join(result, active.stop())
	}
	return result
}
func (s *workspaceEvents) Close() error {
	s.once.Do(func() {
		close(s.closed)
		owner := s.active.owner
		owner.mu.Lock()
		s.active.mu.Lock()
		delete(s.active.observers, s)
		empty := len(s.active.observers) == 0
		s.active.mu.Unlock()
		remove := empty && owner.roots[s.active.root] == s.active
		if remove {
			delete(owner.roots, s.active.root)
		}
		owner.mu.Unlock()
		if remove {
			_ = s.active.stop()
		}
	})
	return nil
}
func (s *workspaceEvents) Next(ctx context.Context) (protocol.Update, error) {
	s.next.Lock()
	defer s.next.Unlock()
	if !s.initial {
		select {
		case <-ctx.Done():
			return protocol.Update{}, ctx.Err()
		case <-s.closed:
			return protocol.Update{}, watchUnavailable()
		case <-s.wake:
		}
		select {
		case <-ctx.Done():
			return protocol.Update{}, ctx.Err()
		case <-s.closed:
			return protocol.Update{}, watchUnavailable()
		case <-s.active.owner.after(200 * time.Millisecond):
		}
	}
	s.initial = false
	s.active.mu.Lock()
	defer s.active.mu.Unlock()
	if s.active.failure != nil {
		return protocol.Update{}, s.active.failure
	}
	select {
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case <-s.closed:
		return protocol.Update{}, watchUnavailable()
	default:
	}
	// Every pending invalidation is represented by this latest reference counter.
	select {
	case <-s.wake:
	default:
	}
	refs := uint64(0)
	if s.repository {
		refs = s.active.references
	}
	return protocol.Update{Kind: "files_changed", Data: protocol.UpdateFilesChangedPayload{Node: s.node, Worktree: s.worktree, References: refs}}, nil
}
