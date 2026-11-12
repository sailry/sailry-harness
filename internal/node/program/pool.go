package program

import (
	"errors"
	"sync"
	"time"
)

// ErrBusy reports that an owner's programs are all in use.
var ErrBusy = errors.New("programs are busy")

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
	return &Pool{Limit: limit, Idle: idle, entries: map[*Entry]struct{}{}}
}

// Lease returns an idle program of owner that match accepts, else starts
// one. At the limit it replaces the owner's longest-idle program; when every
// one is in use it fails with ErrBusy. The caller must Release the entry.
func (pool *Pool) Lease(owner string, match func(*Entry) bool, start func() (*Process, any, error)) (*Entry, error) {
	pool.mu.Lock()
	if pool.closed {
		pool.mu.Unlock()
		return nil, errors.New("programs are stopped")
	}
	var oldest *Entry
	count := 0
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
		if match(entry) {
			entry.leased = true
			pool.mu.Unlock()
			return entry, nil
		}
		if oldest == nil || entry.used.Before(oldest.used) {
			oldest = entry
		}
	}
	if count >= pool.Limit {
		if oldest == nil {
			pool.mu.Unlock()
			return nil, ErrBusy
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
	if err == nil && pool.closed {
		err = errors.New("programs are stopped")
	}
	if err != nil {
		delete(pool.entries, entry)
		if process != nil {
			go process.Kill()
		}
		return nil, err
	}
	entry.Process, entry.Value = process, value
	return entry, nil
}

// Release returns a leased entry. A program that is not kept is killed.
func (pool *Pool) Release(entry *Entry, keep bool) {
	pool.mu.Lock()
	_, known := pool.entries[entry]
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
