package node

import (
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSSHBrowsePoolReuseInvalidationExpiryAndClose(t *testing.T) {
	t.Parallel()
	clock := clockwork.NewFakeClock()
	pool := newSSHBrowsePool(clock)
	pool.maxIdle = 2
	pool.ttl = 30 * time.Second

	p1, p2, p3 := protocol.NewSshID(), protocol.NewSshID(), protocol.NewSshID()
	c1 := &sshBrowseConn{}
	pool.release(p1, 1, c1)

	// Acquire returns the same idle entry for matching (profile, revision).
	if got := pool.acquire(p1, 1); got != c1 {
		t.Fatalf("acquire(p1, 1) = %v, want %v", got, c1)
	}
	if got := pool.acquire(p1, 1); got != nil {
		t.Fatalf("second acquire while checked out = %v, want nil", got)
	}

	// Releasing a newer revision evicts the previous revision.
	pool.release(p1, 1, c1)
	c1v2 := &sshBrowseConn{}
	pool.release(p1, 2, c1v2)
	if got := pool.acquire(p1, 1); got != nil {
		t.Fatalf("stale revision remained pooled: %v", got)
	}
	if got := pool.acquire(p1, 2); got != c1v2 {
		t.Fatalf("acquire(p1, 2) = %v, want %v", got, c1v2)
	}

	// Bounded capacity evicts the least-recently-used entry.
	c2, c3 := &sshBrowseConn{}, &sshBrowseConn{}
	pool.release(p1, 2, c1v2)
	pool.release(p2, 1, c2)
	pool.release(p3, 1, c3)
	if got := pool.acquire(p1, 2); got != nil {
		t.Fatalf("LRU entry was not evicted at maxIdle=2: %v", got)
	}

	// Profile invalidation clears only that profile.
	pool.invalidate(p2)
	if got := pool.acquire(p2, 1); got != nil {
		t.Fatalf("invalidated profile remained pooled: %v", got)
	}

	// Idle TTL reaps expired entries.
	clock.Advance(31 * time.Second)
	if got := pool.acquire(p3, 1); got != nil {
		t.Fatalf("expired entry was not evicted: %v", got)
	}

	// Close stops accepting new idle entries cleanly.
	if err := pool.close(); err != nil {
		t.Fatal(err)
	}
	pool.release(p3, 1, &sshBrowseConn{})
	if got := pool.acquire(p3, 1); got != nil {
		t.Fatalf("closed pool retained entry: %v", got)
	}
}
