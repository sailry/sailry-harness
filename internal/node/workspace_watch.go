package node

import (
	"context"
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/fsnotify/fsnotify"
	"github.com/sailry/sailry-harness/internal/link"
	workspacegit "github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceWatches struct {
	mu     sync.Mutex
	roots  map[string]*workspaceWatch
	closed bool
	git    *workspacegit.Service
	after  func(time.Duration) <-chan time.Time
}
type workspaceWatch struct {
	owner      *workspaceWatches
	root       string
	native     *fsnotify.Watcher
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
	wake       chan struct{}
	closed     chan struct{}
	once       sync.Once
	next       sync.Mutex
	initial    bool
}

func newWorkspaceWatches(git *workspacegit.Service) *workspaceWatches {
	return &workspaceWatches{roots: map[string]*workspaceWatch{}, git: git, after: time.After}
}
func watchUnavailable() error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "file watch is unavailable"}
}

func (w *workspaceWatches) subscribe(ctx context.Context, root string, node protocol.NodeID, worktree protocol.WorktreeID, repository bool) (link.Subscription, error) {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.closed || ctx.Err() != nil {
		return nil, watchUnavailable()
	}
	active := w.roots[root]
	if active == nil {
		if len(w.roots) >= 32 {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "file watch capacity exhausted"}
		}
		filter, err := workspacegit.NewWatchFilter(root)
		if err != nil {
			return nil, err
		}
		native, err := fsnotify.NewWatcher()
		if err != nil {
			return nil, watchUnavailable()
		}
		active = &workspaceWatch{owner: w, root: root, native: native, done: make(chan struct{}), filter: filter, references: 1, observers: map[*workspaceEvents]struct{}{}}
		watched, err := workspacegit.WatchRoots(root)
		if err == nil {
			err = active.addTree(root)
		}
		for _, path := range watched {
			if err != nil {
				break
			}
			rel, e := filepath.Rel(root, path)
			if e != nil || rel == ".." || len(rel) > 3 && rel[:3] == ".."+string(filepath.Separator) {
				err = active.addTree(path)
			}
		}
		if err != nil {
			_ = native.Close()
			return nil, watchUnavailable()
		}
		w.git.Invalidate(root)
		w.roots[root] = active
		go active.run()
	}
	observer := &workspaceEvents{active: active, node: node, worktree: worktree, repository: repository, wake: make(chan struct{}, 1), closed: make(chan struct{}), initial: true}
	active.mu.Lock()
	active.observers[observer] = struct{}{}
	active.mu.Unlock()
	return observer, nil
}
func (w *workspaceWatch) addTree(root string) error {
	return filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.Type()&os.ModeSymlink != 0 {
			return nil
		}
		if entry.IsDir() {
			return w.native.Add(path)
		}
		return nil
	})
}
func (w *workspaceWatch) run() {
	defer close(w.done)
	for {
		select {
		case event, ok := <-w.native.Events:
			if !ok {
				return
			}
			if event.Has(fsnotify.Create) {
				if entry, err := os.Lstat(event.Name); err == nil && entry.IsDir() && entry.Mode()&os.ModeSymlink == 0 {
					if err := w.addTree(event.Name); err != nil {
						w.notify(nil, watchUnavailable(), false)
						return
					}
				}
			}
			w.notify([]string{event.Name}, nil, false)
		case _, ok := <-w.native.Errors:
			if !ok {
				return
			}
			w.notify(nil, watchUnavailable(), false)
			return
		}
	}
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
	w.mu.Unlock()
	if active != nil {
		active.notify(nil, watchUnavailable(), false)
		_ = active.native.Close()
		<-active.done
	}
}
func (w *workspaceWatches) close() error {
	w.mu.Lock()
	w.closed = true
	roots := w.roots
	w.roots = map[string]*workspaceWatch{}
	w.mu.Unlock()
	var result error
	for _, active := range roots {
		active.notify(nil, watchUnavailable(), false)
		result = errors.Join(result, active.native.Close())
		<-active.done
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
			_ = s.active.native.Close()
			<-s.active.done
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
