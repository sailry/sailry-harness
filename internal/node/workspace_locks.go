package node

import (
	"slices"
	"sync"
	"sync/atomic"
)

// workspaceLocks orders durable workspace effects. Effects on one worktree
// root run one at a time and effects on different roots run together. A
// registration, which changes which roots exist, excludes every effect while
// it is recorded.
type workspaceLocks struct {
	registry sync.RWMutex
	// registrations counts the recorded ones, so an effect that waited can
	// tell that its roots may have changed.
	registrations atomic.Uint64
	mu            sync.Mutex
	held          map[string]*rootLock
	// settings orders the saves whose refresh after the commit reads the
	// saved settings back and applies them, so the last save is the applied
	// one. They work in no root.
	settings sync.Mutex
}

type rootLock struct {
	mu   sync.Mutex
	uses int
}

// roots locks the given roots, in one order so that an effect across two
// roots cannot deadlock with its mirror image. A command that works in no
// worktree has an empty root, which is no lock: such commands (a web fetch
// and its cancellation, for one) run together.
func (locks *workspaceLocks) roots(roots ...string) (release func()) {
	roots = slices.Clone(roots)
	slices.Sort(roots)
	roots = slices.Compact(roots)
	if len(roots) > 0 && roots[0] == "" {
		roots = roots[1:]
	}
	entries := make([]*rootLock, len(roots))
	for index, root := range roots {
		locks.mu.Lock()
		if locks.held == nil {
			locks.held = map[string]*rootLock{}
		}
		entry := locks.held[root]
		if entry == nil {
			entry = &rootLock{}
			locks.held[root] = entry
		}
		entry.uses++
		locks.mu.Unlock()
		entry.mu.Lock()
		entries[index] = entry
	}
	return func() {
		for index := len(entries) - 1; index >= 0; index-- {
			entries[index].mu.Unlock()
			locks.mu.Lock()
			if entries[index].uses--; entries[index].uses == 0 {
				delete(locks.held, roots[index])
			}
			locks.mu.Unlock()
		}
	}
}

// effect locks the roots and then joins the effects a registration waits
// for. Nothing is held while it waits for a root.
func (locks *workspaceLocks) effect(roots ...string) (release func()) {
	unlock := locks.roots(roots...)
	locks.registry.RLock()
	return func() {
		locks.registry.RUnlock()
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
