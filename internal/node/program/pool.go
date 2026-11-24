package program

import (
	"context"
	"errors"
	"sync"
	"time"
)

// Pool bounds and reaps the running programs of every owner (a package). A
// caller attaches its own protocol state to an entry as Value.
type Pool struct {
	Limit int
	Idle  time.Duration
	mu    sync.Mutex
	// entries holds every started program, leased or idle.
	entries map[*Entry]struct{}
	closed  bool
	timer   *time.Timer
	// freed is closed and replaced whenever an entry is released or removed,
	// waking leases that wait for a slot.
	freed chan struct{}
}

// Entry is one running program and the state its user keeps with it.
type Entry struct {
	Process *Process
	Value   any
	owner   string
	leased  bool
	used    time.Time
}

func NewPool(limit int, idle time.Duration) *Pool {
	return &Pool{Limit: limit, Idle: idle, entries: map[*Entry]struct{}{}, freed: make(chan struct{})}
}

// signal wakes waiting leases. The caller holds the lock.
func (pool *Pool) signal() {
	close(pool.freed)
	pool.freed = make(chan struct{})
}

// Lease returns the idle program of owner that rank values highest above
// zero, else starts one. At the limit it replaces the owner's longest-idle
// program; when every one is in use it waits for a release until ctx ends.
// limit replaces the pool's Limit for this owner when it is positive.
// Callers hold an entry for one request, so a wait ends when one finishes.
// The caller must Release the entry.
func (pool *Pool) Lease(ctx context.Context, owner string, limit int, rank func(*Entry) int, start func() (*Process, any, error)) (*Entry, error) {
	for {
		entry, freed, err := pool.lease(owner, limit, rank, start)
		if freed == nil {
			return entry, err
		}
		select {
		case <-freed:
		case <-ctx.Done():
			return nil, context.Cause(ctx)
		}
	}
}

// lease is one attempt of Lease. A non-nil channel reports that every
// program is in use and is closed when one is released or removed.
func (pool *Pool) lease(owner string, limit int, rank func(*Entry) int, start func() (*Process, any, error)) (*Entry, <-chan struct{}, error) {
	pool.mu.Lock()
	if pool.closed {
		pool.mu.Unlock()
		return nil, nil, errors.New("programs are stopped")
	}
	var oldest, best *Entry
	count, score := 0, 0
	for entry := range pool.entries {
		if entry.owner != owner {
			continue
		}
		count++
		if entry.leased {
			continue
		}
		select {
		case <-entry.Process.Done():
			delete(pool.entries, entry)
			count--
			continue
		default:
		}
		if value := rank(entry); value > score {
			best, score = entry, value
		}
		if oldest == nil || entry.used.Before(oldest.used) {
			oldest = entry
		}
	}
	if best != nil {
		best.leased = true
		pool.mu.Unlock()
		return best, nil, nil
	}
	if limit <= 0 {
		limit = pool.Limit
	}
	if count >= limit {
		if oldest == nil {
			freed := pool.freed
			pool.mu.Unlock()
			return nil, freed, nil
		}
		delete(pool.entries, oldest)
	} else {
		oldest = nil
	}
	// The slot is held while the program starts.
	entry := &Entry{owner: owner, leased: true}
	pool.entries[entry] = struct{}{}
	pool.mu.Unlock()
	if oldest != nil {
		oldest.Process.Kill()
	}
	process, value, err := start()
	pool.mu.Lock()
	defer pool.mu.Unlock()
	if _, retained := pool.entries[entry]; err == nil && !retained {
		// Invalidation also removes starting entries. A completed launch must
		// not escape the pool after its package has been disabled or replaced.
		err = errors.New("program start was invalidated")
	}
	if err != nil {
		delete(pool.entries, entry)
		pool.signal()
		if process != nil {
			go process.Kill()
		}
		return nil, nil, err
	}
	entry.Process, entry.Value = process, value
	return entry, nil, nil
}

// Holds reports whether an idle, running program of owner satisfies match.
// Its Value is read under the pool's lock, where no lease can change it.
func (pool *Pool) Holds(owner string, match func(*Entry) bool) bool {
	pool.mu.Lock()
	defer pool.mu.Unlock()
	for entry := range pool.entries {
		if entry.owner != owner || entry.leased || entry.Process == nil {
			continue
		}
		select {
		case <-entry.Process.Done():
			continue
		default:
		}
		if match(entry) {
			return true
		}
	}
	return false
}

// Release returns a leased entry. A program that is not kept is killed.
func (pool *Pool) Release(entry *Entry, keep bool) {
	pool.mu.Lock()
	_, known := pool.entries[entry]
	pool.signal()
	if !keep || !known || pool.closed {
		delete(pool.entries, entry)
		pool.mu.Unlock()
		entry.Process.Kill()
		return
	}
	entry.leased, entry.used = false, time.Now()
	if pool.timer == nil {
		pool.timer = time.AfterFunc(pool.Idle, pool.reap)
	}
	pool.mu.Unlock()
}

func (pool *Pool) reap() {
	pool.mu.Lock()
	pool.timer = nil
	var expired []*Entry
	next := time.Duration(0)
	for entry := range pool.entries {
		if entry.leased {
			continue
		}
		if rest := pool.Idle - time.Since(entry.used); rest <= 0 {
			delete(pool.entries, entry)
			expired = append(expired, entry)
		} else if next == 0 || rest < next {
			next = rest
		}
	}
	if next > 0 && !pool.closed {
		pool.timer = time.AfterFunc(next, pool.reap)
	}
	pool.mu.Unlock()
	for _, entry := range expired {
		entry.Process.Kill()
	}
}

// Invalidate kills every program of owner, in use or not.
func (pool *Pool) Invalidate(owner string) {
	pool.stop(func(entry *Entry) bool { return entry.owner == owner }, false)
}

// Close kills every program and refuses new ones.
func (pool *Pool) Close() { pool.stop(func(*Entry) bool { return true }, true) }

func (pool *Pool) stop(selected func(*Entry) bool, final bool) {
	pool.mu.Lock()
	var stopped []*Entry
	for entry := range pool.entries {
		if selected(entry) {
			delete(pool.entries, entry)
			stopped = append(stopped, entry)
		}
	}
	pool.signal()
	if final {
		pool.closed = true
		if pool.timer != nil {
			pool.timer.Stop()
		}
	}
	pool.mu.Unlock()
	for _, entry := range stopped {
		// An entry still starting has no process yet; its Lease kills it.
		if entry.Process != nil {
			entry.Process.Kill()
		}
	}
}
