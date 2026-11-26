package providerstatusdesktop

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type fakeTimer struct {
	mu      sync.Mutex
	due     time.Time
	fn      func()
	stopped bool
	fired   bool
}

func (ft *fakeTimer) Stop() bool {
	ft.mu.Lock()
	defer ft.mu.Unlock()
	if ft.stopped || ft.fired {
		return false
	}
	ft.stopped = true
	return true
}

type fakeClock struct {
	mu     sync.Mutex
	cur    time.Time
	timers []*fakeTimer
}

func newFakeClock(start time.Time) *fakeClock {
	return &fakeClock{cur: start}
}

func (c *fakeClock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.cur
}

func (c *fakeClock) After(d time.Duration, fn func()) timerHandle {
	c.mu.Lock()
	defer c.mu.Unlock()
	ft := &fakeTimer{due: c.cur.Add(d), fn: fn}
	c.timers = append(c.timers, ft)
	return ft
}

func (c *fakeClock) ActiveTimers() int {
	c.mu.Lock()
	defer c.mu.Unlock()
	n := 0
	for _, ft := range c.timers {
		ft.mu.Lock()
		if !ft.stopped && !ft.fired {
			n++
		}
		ft.mu.Unlock()
	}
	return n
}

func (c *fakeClock) Advance(d time.Duration) {
	c.mu.Lock()
	c.cur = c.cur.Add(d)
	var ready []*fakeTimer
	for _, ft := range c.timers {
		ft.mu.Lock()
		if !ft.stopped && !ft.fired && !ft.due.After(c.cur) {
			ft.fired = true
			ready = append(ready, ft)
		}
		ft.mu.Unlock()
	}
	c.mu.Unlock()
	for _, ft := range ready {
		ft.fn()
	}
}

func TestTrackerFreshTTLAndFailureCooldown(t *testing.T) {
	clk := newFakeClock(time.Date(2026, 4, 10, 12, 0, 0, 0, time.UTC))
	var node protocol.NodeID
	node[0] = 7
	prov := protocol.ProviderID(uuid.New())
	cred := protocol.CredentialID(uuid.New())
	key := usageScopeKey{node: node, provider: prov, providerRev: 1, credID: cred, credRev: 1}

	var calls atomic.Int32
	var shouldFail atomic.Bool
	woke := make(chan struct{}, 16)

	tracker := newAccountUsageTracker(t.Context(), trackerDeps{
		now:   clk.Now,
		after: clk.After,
		wake:  func() { woke <- struct{}{} },
		fetch: func(_ context.Context, _ protocol.NodeID, _ protocol.ProviderID, _ uint64) (protocol.ProviderAccountUsage, error) {
			calls.Add(1)
			if shouldFail.Load() {
				return protocol.ProviderAccountUsage{}, errors.New("upstream unavailable")
			}
			return protocol.ProviderAccountUsage{
				Windows: []protocol.ProviderAccountUsageWindow{{Key: "five_hour", Used: f64(25)}},
			}, nil
		},
	})
	defer func() {
		tracker.Close()
		tracker.wait()
	}()

	// Initial observe triggers first fetch.
	if got := tracker.Observe(key, true); got != nil {
		t.Fatalf("expected nil before first fetch completes, got %+v", got)
	}
	<-woke
	if calls.Load() != 1 {
		t.Fatalf("calls = %d, want 1", calls.Load())
	}

	// Within 180s fresh TTL, Observe returns cached snapshot without re-fetching.
	clk.Advance(100 * time.Second)
	got := tracker.Observe(key, true)
	if got == nil || len(got.Windows) != 1 || *got.Windows[0].Used != 25 {
		t.Fatalf("expected cached snapshot, got %+v", got)
	}
	if calls.Load() != 1 {
		t.Fatalf("should not refetch within 180s TTL, calls = %d", calls.Load())
	}

	// After 180s total, deferred timer triggers refresh; simulate failure -> hides stale value and enters 60s cooldown.
	shouldFail.Store(true)
	clk.Advance(80 * time.Second)
	<-woke
	if calls.Load() != 2 {
		t.Fatalf("expected refresh at 180s, calls = %d", calls.Load())
	}
	if got := tracker.Observe(key, true); got != nil {
		t.Fatalf("stale value must be hidden on failure, got %+v", got)
	}

	// Within 60s cooldown, no retry occurs.
	clk.Advance(59 * time.Second)
	if calls.Load() != 2 {
		t.Fatalf("should stay in 60s failure cooldown, calls = %d", calls.Load())
	}

	// At 60s cooldown expiry, recovery fetch succeeds.
	shouldFail.Store(false)
	clk.Advance(1 * time.Second)
	<-woke
	if calls.Load() != 3 {
		t.Fatalf("expected retry after 60s cooldown, calls = %d", calls.Load())
	}
	if got := tracker.Observe(key, true); got == nil || *got.Windows[0].Used != 25 {
		t.Fatalf("expected recovered snapshot, got %+v", got)
	}
}

func TestTrackerPauseWhenHiddenRetainsSnapshotAndStopsTimer(t *testing.T) {
	clk := newFakeClock(time.Date(2026, 4, 10, 12, 0, 0, 0, time.UTC))
	var node protocol.NodeID
	node[0] = 3
	key := usageScopeKey{node: node, provider: protocol.ProviderID(uuid.New()), providerRev: 2, credID: protocol.CredentialID(uuid.New()), credRev: 1}

	var calls atomic.Int32
	woke := make(chan struct{}, 16)
	tracker := newAccountUsageTracker(t.Context(), trackerDeps{
		now:   clk.Now,
		after: clk.After,
		wake:  func() { woke <- struct{}{} },
		fetch: func(context.Context, protocol.NodeID, protocol.ProviderID, uint64) (protocol.ProviderAccountUsage, error) {
			calls.Add(1)
			return protocol.ProviderAccountUsage{
				Windows: []protocol.ProviderAccountUsageWindow{{Key: "seven_day", Used: f64(40)}},
			}, nil
		},
	})
	defer func() {
		tracker.Close()
		tracker.wait()
	}()

	tracker.Observe(key, true)
	<-woke
	if clk.ActiveTimers() != 1 {
		t.Fatalf("expected 1 active refresh timer while visible, got %d", clk.ActiveTimers())
	}

	// First PauseHidden marks frame boundary; second PauseHidden observes entry not rendered -> pauses timer.
	tracker.PauseHidden()
	tracker.PauseHidden()
	if clk.ActiveTimers() != 0 {
		t.Fatalf("expected deferred refresh timer to stop when hidden, active = %d", clk.ActiveTimers())
	}

	// Advancing past 180s while hidden must NOT poll.
	clk.Advance(60 * time.Second)
	if calls.Load() != 1 {
		t.Fatalf("hidden surface should not poll, calls = %d", calls.Load())
	}

	// Becoming visible again within TTL immediately returns retained snapshot and re-arms timer.
	got := tracker.Observe(key, true)
	if got == nil || *got.Windows[0].Used != 40 {
		t.Fatalf("hidden pause must retain snapshot, got %+v", got)
	}
	if calls.Load() != 1 || clk.ActiveTimers() != 1 {
		t.Fatalf("expected retained snapshot with re-armed timer, calls=%d timers=%d", calls.Load(), clk.ActiveTimers())
	}
}

func TestTrackerDiscardsStaleRepliesOnIdentityChangeAndClosesCleanly(t *testing.T) {
	clk := newFakeClock(time.Date(2026, 4, 10, 12, 0, 0, 0, time.UTC))
	var node protocol.NodeID
	node[0] = 9
	prov := protocol.ProviderID(uuid.New())
	cred := protocol.CredentialID(uuid.New())
	key1 := usageScopeKey{node: node, provider: prov, providerRev: 1, credID: cred, credRev: 1}
	key2 := usageScopeKey{node: node, provider: prov, providerRev: 1, credID: cred, credRev: 2}

	firstStarted := make(chan struct{})
	firstCanceled := make(chan struct{})
	releaseFirst := make(chan struct{})
	woke := make(chan struct{}, 16)

	var callNum atomic.Int32
	tracker := newAccountUsageTracker(t.Context(), trackerDeps{
		now:   clk.Now,
		after: clk.After,
		wake:  func() { woke <- struct{}{} },
		fetch: func(ctx context.Context, _ protocol.NodeID, _ protocol.ProviderID, _ uint64) (protocol.ProviderAccountUsage, error) {
			n := callNum.Add(1)
			if n == 1 {
				close(firstStarted)
				<-ctx.Done()
				close(firstCanceled)
				<-releaseFirst
				// Return a stale success even after cancellation to verify seq/key discard.
				return protocol.ProviderAccountUsage{
					Windows: []protocol.ProviderAccountUsageWindow{{Key: "five_hour", Used: f64(99)}},
				}, nil
			}
			return protocol.ProviderAccountUsage{
				Windows: []protocol.ProviderAccountUsageWindow{{Key: "five_hour", Used: f64(10)}},
			}, nil
		},
	})

	tracker.Observe(key1, true)
	<-firstStarted

	// Credential revision changes while first fetch is still in flight.
	tracker.Observe(key2, true)
	<-firstCanceled
	<-woke // second fetch completed

	// Now let first (stale) goroutine return its old value.
	close(releaseFirst)
	// Ensure key2 still holds its own value (10), never overwritten by stale 99.
	got := tracker.Observe(key2, true)
	if got == nil || *got.Windows[0].Used != 10 {
		t.Fatalf("expected key2 snapshot (used=10), got %+v", got)
	}

	// Close must stop all timers and allow all goroutines to exit cleanly.
	tracker.Close()
	tracker.wait()
	if clk.ActiveTimers() != 0 {
		t.Fatalf("expected 0 active timers after Close(), got %d", clk.ActiveTimers())
	}
}

func TestRegisterAndBindWindowMetrics(t *testing.T) {
	var assembly shell.Assembly
	Register(&assembly)

	if len(assembly.Locales) != 1 || len(assembly.MetricContributions) != 1 || len(assembly.PluginControlReplacements) != 2 {
		t.Fatalf("unexpected Register assembly: locales=%d metrics=%d replacements=%d",
			len(assembly.Locales), len(assembly.MetricContributions), len(assembly.PluginControlReplacements))
	}

	var node protocol.NodeID
	node[0] = 0x42
	provID := protocol.ProviderID(uuid.New())
	credID := protocol.CredentialID(uuid.New())
	woke := make(chan struct{}, 8)
	visible := true

	services := viewapi.Services{
		Node: node,
		Wake: func() { woke <- struct{}{} },
		ProviderAccount: func(id protocol.NodeID, provider protocol.ProviderID) (viewapi.ProviderAccount, bool) {
			return viewapi.ProviderAccount{
				Authentication: protocol.AuthenticationClaudeOAuth, ProviderRevision: 3,
				Credential: credID, CredentialKnown: true, CredentialRevision: 5,
			}, id == node && provider == provID
		},
		PrepareOn: func(_ protocol.NodeID, cmd protocol.Command) (protocol.Request, error) {
			return protocol.Request{Command: cmd}, nil
		},
		ExecuteOn: func(_ context.Context, _ protocol.NodeID, req protocol.Request) (protocol.Output, error) {
			payload, ok := req.Command.Data.(*protocol.CommandReadProviderAccountUsagePayload)
			if !ok || req.Command.Kind != "read_provider_account_usage" || payload.Provider != provID || payload.ExpectedRevision != 3 {
				return protocol.Output{}, errors.New("unexpected command payload")
			}
			return protocol.Output{
				Kind: "provider_account_usage",
				Data: protocol.ProviderAccountUsage{
					Windows: []protocol.ProviderAccountUsageWindow{
						{Key: "five_hour", Used: f64(20)},
						{Key: "seven_day", Used: f64(50)},
					},
				},
			}, nil
		},
	}

	ctx := viewapi.WithServices(t.Context(), services)
	binding := assembly.MetricContributions[0].Bind(shell.WindowContext{
		Context: ctx,
		Visible: func() bool { return visible },
	})
	defer binding.Close()

	cat := i18n.New(Locales)
	var leadResult []conversation.Metric
	var providerResult *settings.ChannelUsage
	tester := ui.NewTester(func(c *ui.Context) {
		i18n.Bind(c, cat)
		leadResult = binding.Lead(c, shell.ComposerMetricScope{
			Node:        node.String(),
			Provider:    provID.String(),
			ProviderRev: 3,
			Auth:        protocol.AuthenticationClaudeOAuth,
			Credential:  &protocol.CredentialRef{ID: credID},
			CredRev:     5,
			Connected:   true,
		})
		providerResult = binding.Provider(c, node.String(), provID.String())
	}, 400, 200)
	tester.SetLocale("en")
	tester.Frame()
	<-woke
	tester.Frame()

	if len(leadResult) != 1 || leadResult[0].Value != "50%" {
		t.Fatalf("unexpected bound lead metric: %+v", leadResult)
	}
	if providerResult == nil || len(providerResult.Lines) != 2 || providerResult.Lines[0].Value != "80%" || providerResult.Lines[1].Value != "50%" {
		t.Fatalf("unexpected bound provider usage: %+v", providerResult)
	}

	// Demo scope or hidden window suppresses output cleanly.
	visible = false
	binding.Sync()
	tester.Frame()
	if len(leadResult) != 0 || providerResult != nil {
		t.Fatalf("expected hidden window to suppress metrics: lead=%+v provider=%+v", leadResult, providerResult)
	}
}

func TestLeadAndReconcileBeforeCredentialsLoaded(t *testing.T) {
	var assembly shell.Assembly
	Register(&assembly)

	var node protocol.NodeID
	node[0] = 0x55
	provID := protocol.ProviderID(uuid.New())
	credID := protocol.CredentialID(uuid.New())
	woke := make(chan struct{}, 8)

	var fetchCount atomic.Int32
	var revoked atomic.Bool

	services := viewapi.Services{
		Node: node,
		Wake: func() { woke <- struct{}{} },
		ProviderAccount: func(id protocol.NodeID, provider protocol.ProviderID) (viewapi.ProviderAccount, bool) {
			return viewapi.ProviderAccount{
				Authentication: protocol.AuthenticationChatGptOAuth, ProviderRevision: 2, Credential: credID,
			}, id == node && provider == provID && !revoked.Load()
		},
		PrepareOn: func(_ protocol.NodeID, cmd protocol.Command) (protocol.Request, error) {
			return protocol.Request{Command: cmd}, nil
		},
		ExecuteOn: func(_ context.Context, _ protocol.NodeID, _ protocol.Request) (protocol.Output, error) {
			fetchCount.Add(1)
			return protocol.Output{
				Kind: "provider_account_usage",
				Data: protocol.ProviderAccountUsage{
					Windows: []protocol.ProviderAccountUsageWindow{{Key: "five_hour", Used: f64(30)}},
				},
			}, nil
		},
	}

	ctx := viewapi.WithServices(t.Context(), services)
	binding := assembly.MetricContributions[0].Bind(shell.WindowContext{
		Context: ctx,
		Visible: func() bool { return true },
	})
	defer binding.Close()

	cat := i18n.New(Locales)
	scope := shell.ComposerMetricScope{
		Node:        node.String(),
		Provider:    provID.String(),
		ProviderRev: 2,
		Auth:        protocol.AuthenticationChatGptOAuth,
		Credential:  &protocol.CredentialRef{ID: credID},
		CredRev:     4,
		Connected:   true,
	}

	var leadResult []conversation.Metric
	tester := ui.NewTester(func(c *ui.Context) {
		i18n.Bind(c, cat)
		leadResult = binding.Lead(c, scope)
	}, 400, 200)
	tester.SetLocale("en")

	// Before credential metadata loads, Lead still fetches and displays usage.
	binding.Sync()
	tester.Frame()
	<-woke
	binding.Sync()
	tester.Frame()
	if len(leadResult) != 1 || leadResult[0].Value != "70%" {
		t.Fatalf("expected footer metric before credentials load, got %+v", leadResult)
	}
	if fetchCount.Load() != 1 {
		t.Fatalf("reconcile should keep cached footer entry while credentials are nil, fetches=%d", fetchCount.Load())
	}

	// Once credentials load and report the credential revoked, reconcile evicts it and Lead hides it.
	revoked.Store(true)
	binding.Sync()
	tester.Frame()
	if len(leadResult) != 0 {
		t.Fatalf("revoked credential should hide footer metric, got %+v", leadResult)
	}
}

func TestOnChangeNeverCallsWindowVisibleOffUIThread(t *testing.T) {
	var assembly shell.Assembly
	Register(&assembly)

	var node protocol.NodeID
	node[0] = 0x77
	var onChange func()
	// Plain non-atomic bool mutated on the test's "UI thread" to catch any
	// off-thread window.Visible() call under go test -race.
	uiVisible := true

	services := viewapi.Services{
		Node: node,
		OnChange: func(cb func()) func() {
			onChange = cb
			return func() { onChange = nil }
		},
		ProviderAccount: func(protocol.NodeID, protocol.ProviderID) (viewapi.ProviderAccount, bool) {
			return viewapi.ProviderAccount{}, false
		},
	}

	binding := assembly.MetricContributions[0].Bind(shell.WindowContext{
		Context: viewapi.WithServices(t.Context(), services),
		Visible: func() bool { return uiVisible },
	})
	defer binding.Close()

	done := make(chan struct{})
	go func() {
		defer close(done)
		for i := 0; i < 200; i++ {
			onChange()
		}
	}()
	for i := 0; i < 200; i++ {
		uiVisible = i%2 == 0
		binding.Sync()
	}
	<-done
}
