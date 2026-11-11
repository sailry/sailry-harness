//go:build !darwin

package fswatch

import (
	"io/fs"
	"os"
	"path/filepath"
	"sync"

	"github.com/fsnotify/fsnotify"
)

// Watcher delivers changed paths in batches. Events and Errors close after
// Close.
type Watcher struct {
	Events chan []Event
	// Errors reports a backend failure such as an overflowed event queue;
	// changes may have been missed.
	Errors chan error

	native *fsnotify.Watcher
	done   chan struct{}
	close  sync.Once

	mutex sync.Mutex
	// recursive holds every directory watched as part of a tree; a
	// directory created in one of them joins the tree.
	recursive map[string]bool
	dirs      map[string]bool
}

// New starts a watcher without watches.
func New() (*Watcher, error) {
	native, err := fsnotify.NewWatcher()
	if err != nil {
		return nil, err
	}
	watcher := &Watcher{
		Events: make(chan []Event), Errors: make(chan error),
		native: native, done: make(chan struct{}),
		recursive: map[string]bool{}, dirs: map[string]bool{},
	}
	go watcher.run()
	return watcher, nil
}

func (watcher *Watcher) run() {
	defer close(watcher.Errors)
	defer close(watcher.Events)
	for {
		select {
		case event, open := <-watcher.native.Events:
			if !open {
				return
			}
			found := Event{Path: event.Name}
			watcher.mutex.Lock()
			joins := event.Has(fsnotify.Create) && watcher.recursive[filepath.Dir(event.Name)]
			if event.Has(fsnotify.Remove) || event.Has(fsnotify.Rename) {
				delete(watcher.recursive, event.Name)
				if watcher.dirs[event.Name] {
					delete(watcher.dirs, event.Name)
					found.Lost = true
				}
			}
			watcher.mutex.Unlock()
			if joins {
				if entry, err := os.Lstat(event.Name); err == nil && entry.IsDir() {
					if err := watcher.Tree(event.Name); err != nil && !watcher.fail(err) {
						return
					}
				}
			}
			select {
			case <-watcher.done:
				return
			case watcher.Events <- []Event{found}:
			}
		case err, open := <-watcher.native.Errors:
			if !open || !watcher.fail(err) {
				return
			}
		}
	}
}

func (watcher *Watcher) fail(err error) bool {
	select {
	case <-watcher.done:
		return false
	case watcher.Errors <- err:
		return true
	}
}

// Tree watches root and everything below it, now and later. Symbolic links
// are not followed.
func (watcher *Watcher) Tree(root string) error {
	return filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			if path != root && os.IsNotExist(err) {
				// Removed while walking; its parent reports the removal.
				return nil
			}
			return err
		}
		if !entry.IsDir() {
			return nil
		}
		if err := watcher.native.Add(path); err != nil {
			if path != root && os.IsNotExist(err) {
				return fs.SkipDir
			}
			return err
		}
		watcher.mutex.Lock()
		watcher.recursive[path] = true
		watcher.mutex.Unlock()
		return nil
	})
}

// Dir watches the entries of one directory. root is the directory tree that
// holds it; it matters only on macOS, where one stream on root serves every
// directory added under it.
func (watcher *Watcher) Dir(_, dir string) error {
	watcher.mutex.Lock()
	known := watcher.dirs[dir]
	watcher.mutex.Unlock()
	if known {
		return nil
	}
	if err := watcher.native.Add(dir); err != nil {
		return err
	}
	watcher.mutex.Lock()
	watcher.dirs[dir] = true
	watcher.mutex.Unlock()
	return nil
}

// Drop ends the watch of a directory added with Dir.
func (watcher *Watcher) Drop(dir string) {
	watcher.mutex.Lock()
	known := watcher.dirs[dir]
	delete(watcher.dirs, dir)
	watcher.mutex.Unlock()
	if known {
		_ = watcher.native.Remove(dir)
	}
}

// Close ends every watch and closes Events and Errors.
func (watcher *Watcher) Close() error {
	var err error
	watcher.close.Do(func() {
		close(watcher.done)
		err = watcher.native.Close()
	})
	return err
}
