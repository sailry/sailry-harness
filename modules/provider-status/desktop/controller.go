package providerstatusdesktop

import (
	"context"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	freshTTL        = 180 * time.Second
	failureCooldown = 60 * time.Second
)

// supportsAccountUsage reports whether auth supports native account usage reads.
func supportsAccountUsage(auth protocol.Authentication) bool {
	switch auth {
	case protocol.AuthenticationChatGpt,
		protocol.AuthenticationChatGptOAuth,
		protocol.AuthenticationClaudeOAuth:
		return true
	default:
		return false
	}
}

type usageScopeKey struct {
	node        protocol.NodeID
	provider    protocol.ProviderID
	providerRev uint64
	credID      protocol.CredentialID
	credRev     uint64
}

type slotID struct {
	node     protocol.NodeID
	provider protocol.ProviderID
}

type timerHandle interface {
	Stop() bool
}

type fetchAccountUsageFunc func(ctx context.Context, node protocol.NodeID, provider protocol.ProviderID, expectedRev uint64) (protocol.ProviderAccountUsage, error)

type trackerDeps struct {
	fetch fetchAccountUsageFunc
	wake  func()
	now   func() time.Time
	after func(d time.Duration, fn func()) timerHandle
}

type usageEntry struct {
	key         usageScopeKey
	snapshot    *protocol.ProviderAccountUsage
	refreshAt   time.Time
	pending     bool
	seq         uint64
	cancel      context.CancelFunc
	timer       timerHandle
	timerAt     time.Time
	timerSeq    uint64
	visible     bool
	seenInFrame bool
}

func (e *usageEntry) stopTimer() {
	if e.timer != nil {
		e.timer.Stop()
		e.timer = nil
		e.timerAt = time.Time{}
		e.timerSeq++
	}
}

func (e *usageEntry) cancelFetch() {
	if e.cancel != nil {
		e.cancel()
		e.cancel = nil
	}
	e.pending = false
	e.seq++
}

// accountUsageTracker owns cached account usage snapshots, fresh/cooldown
// timers, and in-flight read contexts for one Desktop window.
type accountUsageTracker struct {
	mu      sync.Mutex
	wg      sync.WaitGroup
	ctx     context.Context
	cancel  context.CancelFunc
	deps    trackerDeps
	entries map[slotID]*usageEntry
	closed  bool
}

func newAccountUsageTracker(parent context.Context, deps trackerDeps) *accountUsageTracker {
	if parent == nil {
		parent = context.Background()
	}
	if deps.now == nil {
		deps.now = time.Now
	}
	if deps.after == nil {
		deps.after = func(d time.Duration, fn func()) timerHandle {
			return time.AfterFunc(d, fn)
		}
	}
	if deps.wake == nil {
		deps.wake = func() {}
	}
	ctx, cancel := context.WithCancel(parent)
	return &accountUsageTracker{
		ctx:     ctx,
		cancel:  cancel,
		deps:    deps,
		entries: map[slotID]*usageEntry{},
	}
}

// Observe returns the current cached snapshot (if any) for key and ensures a
// fetch or deferred refresh timer is active while the surface is visible.
func (t *accountUsageTracker) Observe(key usageScopeKey, connected bool) *protocol.ProviderAccountUsage {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.closed || !connected {
		return nil
	}
	id := slotID{node: key.node, provider: key.provider}
	entry := t.entries[id]
	if entry == nil || entry.key != key {
		if entry != nil {
			entry.stopTimer()
			entry.cancelFetch()
		}
		entry = &usageEntry{key: key}
		t.entries[id] = entry
	}
	entry.visible = true
	entry.seenInFrame = true
	t.scheduleOrFetchLocked(id, entry)
	if entry.snapshot == nil {
		return nil
	}
	cp := cloneAccountUsage(*entry.snapshot)
	return &cp
}

// PauseHidden stops deferred refresh timers for entries that were not rendered
// in the previous frame, while retaining their cached snapshots.
func (t *accountUsageTracker) PauseHidden() {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.closed {
		return
	}
	for _, entry := range t.entries {
		if !entry.seenInFrame {
			entry.visible = false
			entry.stopTimer()
		}
		entry.seenInFrame = false
	}
}

// PauseAll pauses deferred refresh timers for all entries while retaining
// snapshots (for example, while Demo mode is active or the window is hidden).
func (t *accountUsageTracker) PauseAll() {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.closed {
		return
	}
	for _, entry := range t.entries {
		entry.visible = false
		entry.seenInFrame = false
		entry.stopTimer()
	}
}

// Reconcile drops entries whose provider or credential identity no longer
// matches active scopes, canceling any stale in-flight fetches and timers.
func (t *accountUsageTracker) Reconcile(valid func(usageScopeKey) bool) {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.closed {
		return
	}
	for id, entry := range t.entries {
		if !valid(entry.key) {
			entry.stopTimer()
			entry.cancelFetch()
			delete(t.entries, id)
		}
	}
}

// Close cancels all in-flight fetches, stops all timers, and releases entries.
func (t *accountUsageTracker) Close() {
	t.mu.Lock()
	if t.closed {
		t.mu.Unlock()
		return
	}
	t.closed = true
	for id, entry := range t.entries {
		entry.stopTimer()
		entry.cancelFetch()
		delete(t.entries, id)
	}
	cancel := t.cancel
	t.mu.Unlock()
	cancel()
}

// wait blocks until all background fetch goroutines have exited (used in tests).
func (t *accountUsageTracker) wait() {
	t.wg.Wait()
}

func (t *accountUsageTracker) scheduleOrFetchLocked(id slotID, entry *usageEntry) {
	if t.closed || !entry.visible || entry.pending || t.deps.fetch == nil {
		return
	}
	now := t.deps.now()
	if !entry.refreshAt.IsZero() && now.Before(entry.refreshAt) {
		t.deferRefreshLocked(id, entry, entry.refreshAt)
		return
	}
	entry.stopTimer()
	t.startFetchLocked(id, entry)
}

func (t *accountUsageTracker) deferRefreshLocked(id slotID, entry *usageEntry, at time.Time) {
	if t.closed || !entry.visible {
		return
	}
	if entry.timer != nil && !entry.timerAt.After(at) {
		return
	}
	entry.stopTimer()
	delay := max(time.Duration(0), at.Sub(t.deps.now()))
	entry.timerAt = at
	entry.timerSeq++
	seq := entry.timerSeq
	key := entry.key
	entry.timer = t.deps.after(delay, func() {
		t.mu.Lock()
		defer t.mu.Unlock()
		if t.closed {
			return
		}
		cur := t.entries[id]
		if cur != entry || cur.key != key || cur.timerSeq != seq {
			return
		}
		cur.timer = nil
		cur.timerAt = time.Time{}
		if !cur.visible || cur.pending {
			return
		}
		if t.deps.now().Before(cur.refreshAt) {
			t.deferRefreshLocked(id, cur, cur.refreshAt)
			return
		}
		t.startFetchLocked(id, cur)
	})
}

func (t *accountUsageTracker) startFetchLocked(id slotID, entry *usageEntry) {
	reqCtx, cancel := context.WithCancel(t.ctx)
	entry.pending = true
	entry.cancel = cancel
	entry.seq++
	seq := entry.seq
	key := entry.key

	t.wg.Add(1)
	go func() {
		defer t.wg.Done()
		defer cancel()
		usage, err := t.deps.fetch(reqCtx, key.node, key.provider, key.providerRev)

		t.mu.Lock()
		cur := t.entries[id]
		if t.closed || cur != entry || cur.key != key || cur.seq != seq {
			t.mu.Unlock()
			return
		}
		cur.pending = false
		cur.cancel = nil
		now := t.deps.now()
		if err != nil {
			// Hide stale value on failure and enter 60s cooldown; never emit error toasts.
			cur.snapshot = nil
			cur.refreshAt = now.Add(failureCooldown)
		} else {
			cp := cloneAccountUsage(usage)
			cur.snapshot = &cp
			cur.refreshAt = now.Add(freshTTL)
		}
		if cur.visible {
			t.deferRefreshLocked(id, cur, cur.refreshAt)
		}
		wake := t.deps.wake
		t.mu.Unlock()

		wake()
	}()
}

func cloneAccountUsage(u protocol.ProviderAccountUsage) protocol.ProviderAccountUsage {
	out := protocol.ProviderAccountUsage{
		Windows: make([]protocol.ProviderAccountUsageWindow, len(u.Windows)),
	}
	for i, w := range u.Windows {
		cp := protocol.ProviderAccountUsageWindow{Key: w.Key}
		if w.Used != nil {
			v := *w.Used
			cp.Used = &v
		}
		if w.ResetMs != nil {
			v := *w.ResetMs
			cp.ResetMs = &v
		}
		out.Windows[i] = cp
	}
	if u.Credits != nil {
		c := &protocol.ProviderAccountUsageCredits{}
		if u.Credits.Balance != nil {
			v := *u.Credits.Balance
			c.Balance = &v
		}
		if u.Credits.Unlimited != nil {
			v := *u.Credits.Unlimited
			c.Unlimited = &v
		}
		if u.Credits.Available != nil {
			v := *u.Credits.Available
			c.Available = &v
		}
		out.Credits = c
	}
	if u.Extra != nil {
		e := &protocol.ProviderAccountUsageExtra{}
		if u.Extra.Used != nil {
			v := *u.Extra.Used
			e.Used = &v
		}
		if u.Extra.Limit != nil {
			v := *u.Extra.Limit
			e.Limit = &v
		}
		if u.Extra.Currency != nil {
			v := *u.Extra.Currency
			e.Currency = &v
		}
		if u.Extra.Enabled != nil {
			v := *u.Extra.Enabled
			e.Enabled = &v
		}
		out.Extra = e
	}
	return out
}
