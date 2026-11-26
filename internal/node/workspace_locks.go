package node

import (
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/sailry/sailry-harness/internal/node/files"
	"golang.org/x/text/cases"
	"golang.org/x/text/unicode/norm"
)

// workspaceLocks orders effects on overlapping files or whole worktrees.
// A registration excludes every effect while it changes the set of roots.
type workspaceLocks struct {
	registry sync.RWMutex
	// registrations counts the recorded ones, so an effect that waited can
	// tell that its roots may have changed.
	registrations atomic.Uint64
	mu            sync.Mutex
	held          map[*workspaceReservation]struct{}
	// settings orders the saves whose refresh after the commit reads the
	// saved settings back and applies them, so the last save is the applied
	// one. They work in no root.
	settings sync.Mutex
}

type workspaceReservation struct {
	paths []string
	done  chan struct{}
}

// workspaceFileKey is only an ordering key. Operations still use the original
// spelling with the confined file API. Fold equivalent names conservatively
// because different mounts can have different case and normalization rules.
func workspaceFileKey(root, name string) string {
	if root == "" && name == "" {
		return ""
	}
	if !filepath.IsAbs(name) {
		name = filepath.Join(root, filepath.FromSlash(name))
	}
	// On Windows, resolving the existing leaf expands its 8.3 alias too.
	// File access itself still rejects symlinks independently of this key.
	name = resolvedPath(filepath.Clean(name))
	return norm.NFC.String(cases.Fold().String(norm.NFC.String(filepath.ToSlash(name))))
}

func workspacePathsOverlap(a, b string) bool {
	return a == b || strings.HasPrefix(a, strings.TrimSuffix(b, "/")+"/") || strings.HasPrefix(b, strings.TrimSuffix(a, "/")+"/")
}

// Reserve the entire set at once. A later overlapping operation waits for
// its predecessors, including waiting ones, while independent paths proceed.
func (locks *workspaceLocks) paths(keys []string) func() {
	if len(keys) == 0 {
		return func() {}
	}
	reserved := &workspaceReservation{paths: keys, done: make(chan struct{})}
	var wait []chan struct{}
	locks.mu.Lock()
	if locks.held == nil {
		locks.held = make(map[*workspaceReservation]struct{})
	}
	for earlier := range locks.held {
		for _, key := range keys {
			if slices.ContainsFunc(earlier.paths, func(path string) bool { return workspacePathsOverlap(key, path) }) {
				wait = append(wait, earlier.done)
				break
			}
		}
	}
	locks.held[reserved] = struct{}{}
	locks.mu.Unlock()
	for _, done := range wait {
		<-done
	}
	return func() {
		locks.mu.Lock()
		delete(locks.held, reserved)
		close(reserved.done)
		locks.mu.Unlock()
	}
}

// A command that operates in no worktree has no root to reserve.
func (locks *workspaceLocks) roots(roots ...string) func() {
	var keys []string
	for _, root := range roots {
		if key := workspaceFileKey(root, ""); key != "" && !slices.Contains(keys, key) {
			keys = append(keys, key)
		}
	}
	return locks.paths(keys)
}

// Paths are relative to the actual filesystem root used to prepare the change,
// which can be a volume root on calls with external paths.
func preparedFileKeys(root string, prepared *files.Prepared) []string {
	if prepared == nil || prepared.Rejected != nil {
		return nil
	}
	keys := make([]string, 0, len(prepared.Changes))
	for _, change := range prepared.Changes {
		keys = append(keys, workspaceFileKey(root, change.Path))
		if change.Destination != nil {
			keys = append(keys, workspaceFileKey(root, *change.Destination))
		}
	}
	return keys
}

func (locks *workspaceLocks) fileEffect(root string, prepared *files.Prepared) func() {
	if keys := preparedFileKeys(root, prepared); len(keys) != 0 {
		return locks.joinEffect(locks.paths(keys))
	}
	return locks.effect(root)
}

func (locks *workspaceLocks) effect(roots ...string) func() {
	return locks.joinEffect(locks.roots(roots...))
}

// Joining after the paths are available lets registrations pass queued effects.
func (locks *workspaceLocks) joinEffect(unlock func()) func() {
	locks.registry.RLock()
	return func() {
		locks.registry.RUnlock()
		unlock()
	}
}

// change is the effect that creates or removes a worktree: Git works on disk
// and the row follows. It holds its roots and joins the effects, from Git's
// work to the row, so a slow checkout stalls those roots and not every effect
// on the Node. A project command takes the registration and so still cannot
// move or remove the project between the two. Its release counts as a
// registration before the roots open, so an effect that waited for a removed
// root resolves it again and finds it gone.
//
// Rust marks the worktree and its project busy in a table of pending removals
// instead; the locks here already order the same commands. While a project
// command waits for a change to end, new effects wait behind it, as they do
// behind any running effect.
func (locks *workspaceLocks) change(roots ...string) (release func()) {
	unlock := locks.effect(roots...)
	return func() {
		locks.registrations.Add(1)
		unlock()
	}
}

// registration excludes every effect until it is released.
func (locks *workspaceLocks) registration() (release func()) {
	locks.registry.Lock()
	return func() {
		locks.registrations.Add(1)
		locks.registry.Unlock()
	}
}
