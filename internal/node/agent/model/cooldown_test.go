package model

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type gatedSleep struct {
	duration time.Duration
	release  chan struct{}
}

type sharedTestClock struct {
	mu      sync.Mutex
	now     time.Time
	waiters chan gatedSleep
}

func newSharedTestClock(start time.Time) *sharedTestClock {
	return &sharedTestClock{now: start, waiters: make(chan gatedSleep, 8)}
}

func (c *sharedTestClock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.now
}

func (c *sharedTestClock) Advance(d time.Duration) {
	c.mu.Lock()
	c.now = c.now.Add(d)
	c.mu.Unlock()
}

func (c *sharedTestClock) Sleep(ctx context.Context, d time.Duration) error {
	release := make(chan struct{})
	select {
	case c.waiters <- gatedSleep{duration: d, release: release}:
	case <-ctx.Done():
		return ctx.Err()
	}
	select {
	case <-release:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func TestShared429CooldownBlocksSameCredentialNotOtherCredentials(t *testing.T) {
	t.Parallel()
	start := time.Date(2026, 11, 26, 4, 0, 0, 0, time.UTC)
	timer := newSharedTestClock(start)
	cooldowns := NewCooldowns()

	secretA := protocol.NewSecret("sk-shared-account-a")
	secretC := protocol.NewSecret("sk-independent-account-c")
	providerA := protocol.ConversationProvider{ID: protocol.NewProviderID(), Endpoint: "https://gateway.invalid/v1"}
	keyA := CredentialCooldownKey(Binding{Provider: providerA, Secret: &secretA, Config: protocol.SessionConfig{Model: "gpt-test"}})
	keyB := CredentialCooldownKey(Binding{Provider: providerA, Secret: &secretA, Config: protocol.SessionConfig{Model: "gpt-test"}})
	keyC := CredentialCooldownKey(Binding{Provider: providerA, Secret: &secretC, Config: protocol.SessionConfig{Model: "gpt-test"}})
	if keyA != keyB {
		t.Fatalf("same credential produced different cooldown keys: %q vs %q", keyA, keyB)
	}
	if keyA == keyC {
		t.Fatal("different credentials collided on the same cooldown key")
	}
	if strings.Contains(keyA, "sk-shared-account-a") || strings.Contains(keyC, "sk-independent-account-c") {
		t.Fatalf("cooldown key leaked raw secret: %q", keyA)
	}

	var callsA, callsB, callsC atomic.Int32
	modelA := &policyModel{
		onCall: func(int) { callsA.Add(1) },
		responses: [][]fantasy.StreamPart{
			{{
				Type: fantasy.StreamPartTypeError,
				Error: &fantasy.ProviderError{
					StatusCode:      http.StatusTooManyRequests,
					Message:         "rate limited",
					ResponseHeaders: map[string]string{"Retry-After": "45"},
				},
			}},
			{textPart("A recovered"), finishPart()},
		},
	}
	modelB := &policyModel{
		onCall:    func(int) { callsB.Add(1) },
		responses: [][]fantasy.StreamPart{{textPart("B succeeded after cooldown"), finishPart()}},
	}
	modelC := &policyModel{
		onCall:    func(int) { callsC.Add(1) },
		responses: [][]fantasy.StreamPart{{textPart("C immediate"), finishPart()}},
	}

	// Start Request A: receives 429 + Retry-After: 45 and enters 45s wait.
	errA := make(chan error, 1)
	go func() {
		errA <- Run(WithCooldown(t.Context(), cooldowns, keyA), modelA, fantasy.Call{}, Main, timer, nil, nil, nil)
	}()
	waitA := <-timer.waiters
	if waitA.duration != 45*time.Second {
		t.Fatalf("request A wait = %s, want 45s", waitA.duration)
	}
	if callsA.Load() != 1 {
		t.Fatalf("request A calls = %d, want 1", callsA.Load())
	}

	// Advance clock by 5 seconds while A is cooling down, then start Request B
	// (same credential) and Request C (different credential).
	timer.Advance(5 * time.Second)

	// Request C (different credential) must proceed immediately without waiting.
	if err := Run(WithCooldown(t.Context(), cooldowns, keyC), modelC, fantasy.Call{}, Main, timer, nil, nil, nil); err != nil {
		t.Fatalf("request C with different credential failed: %v", err)
	}
	if callsC.Load() != 1 {
		t.Fatalf("request C calls = %d, want 1", callsC.Load())
	}

	// Request B (same credential) must wait for the remaining 40s BEFORE hitting the gateway.
	errB := make(chan error, 1)
	var attemptsB []Attempt
	go func() {
		errB <- Run(WithCooldown(t.Context(), cooldowns, keyB), modelB, fantasy.Call{}, Main, timer, func(a Attempt) error {
			attemptsB = append(attemptsB, a)
			return nil
		}, nil, nil)
	}()
	waitB := <-timer.waiters
	if waitB.duration != 40*time.Second {
		t.Fatalf("request B wait = %s, want remaining 40s", waitB.duration)
	}
	if callsB.Load() != 0 {
		t.Fatalf("request B hit gateway %d times during active 429 cooldown", callsB.Load())
	}

	// Advance clock to t+45s and release both waiters; both must succeed cleanly.
	timer.Advance(40 * time.Second)
	close(waitA.release)
	close(waitB.release)

	if err := <-errA; err != nil {
		t.Fatalf("request A failed after cooldown: %v", err)
	}
	if err := <-errB; err != nil {
		t.Fatalf("request B failed after cooldown: %v", err)
	}
	if callsA.Load() != 2 || callsB.Load() != 1 || len(attemptsB) != 1 || attemptsB[0].Failure != nil {
		t.Fatalf("post-cooldown counts wrong: callsA=%d callsB=%d attemptsB=%+v", callsA.Load(), callsB.Load(), attemptsB)
	}
}

func TestShared429CooldownCancelsCleanlyWhileWaiting(t *testing.T) {
	t.Parallel()
	start := time.Date(2026, 11, 26, 4, 0, 0, 0, time.UTC)
	timer := newSharedTestClock(start)
	cooldowns := NewCooldowns()
	cooldowns.Observe("origin|cred:test", start.Add(45*time.Second), start)

	ctx, cancel := context.WithCancel(WithCooldown(t.Context(), cooldowns, "origin|cred:test"))
	var calls atomic.Int32
	fake := &policyModel{
		onCall:    func(int) { calls.Add(1) },
		responses: [][]fantasy.StreamPart{{textPart("unreachable"), finishPart()}},
	}
	done := make(chan error, 1)
	go func() {
		done <- Run(ctx, fake, fantasy.Call{}, Main, timer, nil, nil, nil)
	}()
	<-timer.waiters
	cancel()
	err := <-done
	if failure := Classify(err); failure == nil || failure.Kind != Cancelled {
		t.Fatalf("cancelled cooldown waiter error = %v, want Cancelled", err)
	}
	if calls.Load() != 0 {
		t.Fatalf("cancelled waiter still hit model %d times", calls.Load())
	}
}

func TestRetryAfterMsHeaderIsHonoredOnSDKFailure(t *testing.T) {
	t.Parallel()
	failure := Classify(&fantasy.ProviderError{
		StatusCode:      http.StatusTooManyRequests,
		Message:         "slow down",
		ResponseHeaders: map[string]string{"Retry-After-Ms": "2500"},
	})
	now := time.Date(2026, 11, 26, 4, 0, 0, 0, time.UTC)
	if got := retryHint(failure, now); got != 2500*time.Millisecond {
		t.Fatalf("retryHint(Retry-After-Ms) = %s, want 2.5s", got)
	}
}

func TestCooldownsBoundsAndCredentialKeyGrouping(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 11, 26, 4, 0, 0, 0, time.UTC)
	c := NewCooldowns()
	// Capped to retryWaitBudget and never shortened.
	c.Observe("k", now.Add(time.Hour), now)
	until, ok := c.Until("k", now)
	if !ok || until != now.Add(retryWaitBudget) {
		t.Fatalf("uncapped cooldown until = %v (ok=%v), want %v", until, ok, now.Add(retryWaitBudget))
	}
	c.Observe("k", now.Add(5*time.Second), now)
	until, _ = c.Until("k", now)
	if until != now.Add(retryWaitBudget) {
		t.Fatalf("shorter observe shortened cooldown to %v", until)
	}
	// Bounded map capacity.
	for i := range maxCooldownEntries + 32 {
		c.Observe(fmt.Sprintf("key-%d", i), now.Add(time.Duration(i+1)*time.Second), now)
	}
	c.mu.Lock()
	size := len(c.entries)
	c.mu.Unlock()
	if size > maxCooldownEntries {
		t.Fatalf("cooldowns map grew to %d, want <= %d", size, maxCooldownEntries)
	}
	// Clear drops all entries.
	c.Clear()
	if _, ok := c.Until("k", now); ok {
		t.Fatal("Clear did not remove active cooldown")
	}

	// OAuth account credential identity stays stable across access token rotations.
	ref := protocol.CredentialRef{Node: protocol.NodeID{1}, ID: protocol.NewCredentialID()}
	tok1, tok2 := protocol.NewSecret("access-token-v1"), protocol.NewSecret("access-token-v2")
	provider := protocol.ConversationProvider{ID: protocol.NewProviderID(), Endpoint: "https://api.example.invalid/v1"}
	k1 := CredentialCooldownKey(Binding{Provider: provider, Config: protocol.SessionConfig{Credential: &ref}, Secret: &tok1})
	k2 := CredentialCooldownKey(Binding{Provider: provider, Config: protocol.SessionConfig{Credential: &ref}, Secret: &tok2})
	if k1 != k2 {
		t.Fatalf("same OAuth account rotated token changed cooldown key: %q vs %q", k1, k2)
	}
}

func TestSDKHTTP429SharesRetryAfterAcrossConcurrentRequests(t *testing.T) {
	t.Parallel()
	success := fixture(t, "responses-text.sse")
	var hits atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		_, _ = io.Copy(io.Discard, request.Body)
		if request.Header.Get("Authorization") == "Bearer key-other" {
			writer.Header().Set("Content-Type", "text/event-stream")
			_, _ = writer.Write(success)
			return
		}
		if hits.Add(1) == 1 {
			writer.Header().Set("Content-Type", "application/json")
			writer.Header().Set("Retry-After", "45")
			writer.WriteHeader(http.StatusTooManyRequests)
			_, _ = io.WriteString(writer, `{"error":{"code":"rate_limit_exceeded","message":"Rate limit reached"}}`)
			return
		}
		writer.Header().Set("Content-Type", "text/event-stream")
		_, _ = writer.Write(success)
	}))
	t.Cleanup(server.Close)

	start := time.Date(2026, 11, 26, 4, 30, 0, 0, time.UTC)
	timer := newSharedTestClock(start)
	cooldowns := NewCooldowns()

	secretShared := protocol.NewSecret("key-shared")
	secretOther := protocol.NewSecret("key-other")
	provider := protocol.ConversationProvider{ID: protocol.NewProviderID(), Endpoint: server.URL}
	keyShared := CredentialCooldownKey(Binding{Provider: provider, Secret: &secretShared, Config: protocol.SessionConfig{Model: "fixture"}})
	keyOther := CredentialCooldownKey(Binding{Provider: provider, Secret: &secretOther, Config: protocol.SessionConfig{Model: "fixture"}})

	modelA, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "key-shared"})
	if err != nil {
		t.Fatal(err)
	}
	modelB, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "key-shared"})
	if err != nil {
		t.Fatal(err)
	}
	modelC, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "key-other"})
	if err != nil {
		t.Fatal(err)
	}

	errA := make(chan error, 1)
	go func() {
		errA <- Run(WithCooldown(t.Context(), cooldowns, keyShared), modelA, requestCall(t, "responses"), Main, timer, nil, nil, nil)
	}()
	waitA := <-timer.waiters
	if waitA.duration != 45*time.Second || hits.Load() != 1 {
		t.Fatalf("HTTP 429 request A: wait=%s hits=%d", waitA.duration, hits.Load())
	}

	timer.Advance(10 * time.Second)
	// Request C with a different API key bypasses the shared cooldown immediately.
	if err := Run(WithCooldown(t.Context(), cooldowns, keyOther), modelC, requestCall(t, "responses"), Main, timer, nil, nil, nil); err != nil {
		t.Fatalf("different-credential SDK request failed: %v", err)
	}

	// Request B with the same API key waits the remaining 35s without hitting the HTTP server.
	errB := make(chan error, 1)
	go func() {
		errB <- Run(WithCooldown(t.Context(), cooldowns, keyShared), modelB, requestCall(t, "responses"), Main, timer, nil, nil, nil)
	}()
	waitB := <-timer.waiters
	if waitB.duration != 35*time.Second {
		t.Fatalf("same-credential request B wait = %s, want 35s", waitB.duration)
	}
	if hits.Load() != 1 {
		t.Fatalf("same-credential request B hit the HTTP gateway during 429 cooldown: hits=%d", hits.Load())
	}

	timer.Advance(35 * time.Second)
	close(waitA.release)
	close(waitB.release)
	if err := <-errA; err != nil {
		t.Fatalf("request A after cooldown: %v", err)
	}
	if err := <-errB; err != nil {
		t.Fatalf("request B after cooldown: %v", err)
	}
	if hits.Load() != 3 {
		t.Fatalf("total shared-credential HTTP hits = %d, want 3", hits.Load())
	}
}

func TestShared429CooldownStaggersMultipleWaiters(t *testing.T) {
	t.Parallel()
	start := time.Date(2026, 11, 26, 5, 0, 0, 0, time.UTC)
	cooldowns := NewCooldowns()
	cooldowns.Observe("origin|cred:shared", start.Add(30*time.Second), start)

	// Waiters claiming the same cooldown window receive deterministic staggered
	// wakeups starting at 0, capped at maxCooldownStagger.
	wantStaggers := []time.Duration{
		0,
		250 * time.Millisecond,
		500 * time.Millisecond,
		750 * time.Millisecond,
		1000 * time.Millisecond,
		1250 * time.Millisecond,
		1500 * time.Millisecond,
		1750 * time.Millisecond,
		2000 * time.Millisecond,
		2000 * time.Millisecond,
	}
	for i, want := range wantStaggers {
		until, stagger, ok := cooldowns.WaitWindow("origin|cred:shared", start)
		if !ok || until != start.Add(30*time.Second) || stagger != want {
			t.Fatalf("waiter %d: until=%v stagger=%s ok=%v, want stagger=%s", i, until, stagger, ok, want)
		}
	}

	// Extending the cooldown starts a fresh stagger sequence.
	cooldowns.Observe("origin|cred:shared", start.Add(45*time.Second), start)
	_, stagger, ok := cooldowns.WaitWindow("origin|cred:shared", start)
	if !ok || stagger != 0 {
		t.Fatalf("extended cooldown first waiter stagger = %s (ok=%v), want 0", stagger, ok)
	}

	// Tight remaining budget/deadline headroom clamps stagger rather than failing with WaitExhausted.
	now := time.Now()
	timer := newSharedTestClock(now)
	cooldowns.Observe("origin|cred:tight", now.Add(10*time.Second), now)
	// Claim slot 0 so the next waiter gets a 250ms stagger.
	cooldowns.WaitWindow("origin|cred:tight", now)

	ctx, cancel := context.WithDeadline(WithCooldown(t.Context(), cooldowns, "origin|cred:tight"), now.Add(10*time.Second+50*time.Millisecond))
	defer cancel()
	fake := &policyModel{responses: [][]fantasy.StreamPart{{textPart("ok"), finishPart()}}}
	done := make(chan error, 1)
	go func() {
		done <- Run(ctx, fake, fantasy.Call{}, Main, timer, nil, nil, nil)
	}()
	wait := <-timer.waiters
	if wait.duration < 10*time.Second || wait.duration > 10*time.Second+50*time.Millisecond {
		t.Fatalf("staggered wait = %s, want clamped within [10s, 10.05s]", wait.duration)
	}
	timer.Advance(wait.duration)
	close(wait.release)
	if err := <-done; err != nil {
		t.Fatalf("staggered waiter with tight deadline failed: %v", err)
	}
}
