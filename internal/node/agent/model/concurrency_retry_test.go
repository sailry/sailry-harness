package model

import (
	"context"
	"errors"
	"net/http"
	"sync"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	"charm.land/fantasy"
)

func TestConcurrencyLimitClassification(t *testing.T) {
	t.Parallel()
	for _, tc := range []struct {
		name               string
		status             int
		body               string
		wantKind           FailureKind
		wantConcurrencyLim bool
	}{
		{
			name:               "http200 sse concurrency limit message",
			status:             http.StatusOK,
			body:               `{"error":{"message":"Concurrency limit exceeded for user, please retry later"}}`,
			wantKind:           RateLimited,
			wantConcurrencyLim: true,
		},
		{
			name:               "http400 concurrency limit message",
			status:             http.StatusBadRequest,
			body:               `{"error":{"code":"concurrency_limit_exceeded","message":"Concurrency limit exceeded for user, please retry later"}}`,
			wantKind:           RateLimited,
			wantConcurrencyLim: true,
		},
		{
			name:               "numeric code 1302",
			status:             http.StatusTooManyRequests,
			body:               `{"error":{"code":1302,"message":"rate limit reached"}}`,
			wantKind:           RateLimited,
			wantConcurrencyLim: true,
		},
		{
			name:               "resource exhausted code",
			status:             http.StatusOK,
			body:               `{"error":{"code":"RESOURCE_EXHAUSTED","message":"quota busy"}}`,
			wantKind:           RateLimited,
			wantConcurrencyLim: false,
		},
		{
			name:               "chinese concurrency message",
			status:             http.StatusTooManyRequests,
			body:               `{"error":{"message":"当前账号并发数超过上限，请稍后重试"}}`,
			wantKind:           RateLimited,
			wantConcurrencyLim: true,
		},
		{
			name:               "too many concurrent requests",
			status:             http.StatusServiceUnavailable,
			body:               `{"error":{"message":"Too many concurrent requests for this account"}}`,
			wantKind:           RateLimited,
			wantConcurrencyLim: true,
		},
		{
			name:               "auth 401 keeps precedence",
			status:             http.StatusUnauthorized,
			body:               `{"error":{"message":"Concurrency limit exceeded"}}`,
			wantKind:           Authentication,
			wantConcurrencyLim: false,
		},
		{
			name:               "context overflow keeps precedence",
			status:             http.StatusBadRequest,
			body:               `{"error":{"code":"context_length_exceeded","message":"Concurrency limit exceeded"}}`,
			wantKind:           ContextOverflow,
			wantConcurrencyLim: false,
		},
	} {
		t.Run(tc.name, func(t *testing.T) {
			failure := Classify(&fantasy.ProviderError{StatusCode: tc.status, ResponseBody: []byte(tc.body)})
			if failure.Kind != tc.wantKind || failure.ConcurrencyLimited() != tc.wantConcurrencyLim {
				t.Fatalf("got kind=%s concurrency=%v retryable=%v, want kind=%s concurrency=%v",
					failure.Kind, failure.ConcurrencyLimited(), failure.Retryable(), tc.wantKind, tc.wantConcurrencyLim)
			}
			if tc.wantKind == RateLimited && (!failure.Retryable() || !failure.overloaded) {
				t.Fatalf("RateLimited must be retryable and overloaded: %+v", failure)
			}
		})
	}
}

type concurrencyGatedModel struct {
	fantasy.LanguageModel
	maxActive    int32
	active       atomic.Int32
	calls        atomic.Int32
	rejects      atomic.Int32
	streamDelay  time.Duration
	firstStarted chan struct{}
	firstOnce    sync.Once
}

func (m *concurrencyGatedModel) Stream(ctx context.Context, _ fantasy.Call) (fantasy.StreamResponse, error) {
	m.calls.Add(1)
	cur := m.active.Add(1)
	if cur > m.maxActive {
		m.active.Add(-1)
		m.rejects.Add(1)
		return nil, &fantasy.ProviderError{
			StatusCode:   http.StatusOK,
			ResponseBody: []byte(`{"error":{"message":"Concurrency limit exceeded for user, please retry later"}}`),
		}
	}
	m.firstOnce.Do(func() { close(m.firstStarted) })
	return func(yield func(fantasy.StreamPart) bool) {
		defer m.active.Add(-1)
		select {
		case <-time.After(m.streamDelay):
		case <-ctx.Done():
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: ctx.Err()})
			return
		}
		if !yield(textPart("ok")) {
			return
		}
		yield(finishPart())
	}, nil
}

func TestAdaptiveConcurrencySlotQueuesSiblingsWithoutBurningRetries(t *testing.T) {
	t.Parallel()
	synctest.Test(t, func(t *testing.T) {
		cooldowns := NewCooldowns()
		const key = "https://gateway.invalid|secret:shared"
		upstream := &concurrencyGatedModel{
			maxActive:    1,
			streamDelay:  2 * time.Second,
			firstStarted: make(chan struct{}),
		}

		errs := make([]error, 4)
		var wg sync.WaitGroup
		wg.Add(4)

		// Start request #0 first so it holds the single upstream slot.
		go func() {
			defer wg.Done()
			errs[0] = Run(WithCooldown(t.Context(), cooldowns, key), upstream, fantasy.Call{}, Main, testSystemClock{}, nil, nil, nil)
		}()
		<-upstream.firstStarted

		// Launch 3 concurrent sibling subagents while #0 is streaming.
		for i := 1; i < 4; i++ {
			go func(idx int) {
				defer wg.Done()
				errs[idx] = Run(WithCooldown(t.Context(), cooldowns, key), upstream, fantasy.Call{}, Main, testSystemClock{}, nil, nil, nil)
			}(i)
		}
		wg.Wait()

		for i, err := range errs {
			if err != nil {
				t.Fatalf("request %d failed under concurrency limit: %v", i, err)
			}
		}
		if limit := cooldowns.SlotLimit(key); limit != 2 {
			// Started clamped at 1, then 4 consecutive successes probed limit to 2.
			t.Fatalf("slot limit after 4 successes = %d, want 2", limit)
		}
		if rejects := upstream.rejects.Load(); rejects > 3 {
			t.Fatalf("too many upstream concurrency rejections = %d (expected <= 3 initial collisions before local queueing)", rejects)
		}
		if cooldowns.InFlight(key) != 0 {
			t.Fatalf("leaked in-flight slots = %d", cooldowns.InFlight(key))
		}
	})
}

type gatedThrottleError struct {
	error
	once        sync.Once
	classifying chan struct{}
	release     chan struct{}
}

func (e *gatedThrottleError) Unwrap() error {
	e.once.Do(func() {
		close(e.classifying)
		<-e.release
	})
	return e.error
}

func TestRetryAfterPublishedBeforeSlotReleaseAndRecheckedByWaiters(t *testing.T) {
	t.Parallel()
	synctest.Test(t, func(t *testing.T) {
		cooldowns := NewCooldowns()
		const key = "gateway|credential"
		ctx := WithCooldown(t.Context(), cooldowns, key)
		// Start clamped to one slot, without any existing cooldown.
		release, acquired, err := cooldowns.AcquireSlot(ctx, key, time.Now(), 0)
		if err != nil || !acquired {
			t.Fatalf("could not acquire seed slot: %t, %v", acquired, err)
		}
		cooldowns.ObserveThrottle(key, &Failure{Kind: RateLimited, concurrencyLimited: true}, time.Now(), 0)
		release()

		blocked := &gatedThrottleError{
			error: &fantasy.ProviderError{
				StatusCode:      http.StatusTooManyRequests,
				ResponseHeaders: map[string]string{"Retry-After": "45"},
			},
			classifying: make(chan struct{}),
			release:     make(chan struct{}),
		}
		allowClassification := sync.OnceFunc(func() { close(blocked.release) })
		defer allowClassification()
		start, reject := make(chan struct{}), make(chan struct{})
		allowRejection := sync.OnceFunc(func() { close(reject) })
		defer allowRejection()
		first := &policyModel{
			startErrors: []error{blocked},
			onCall:      func(int) { close(start); <-reject },
		}
		// End the failing request after its observation to isolate the waiter.
		stopAfterThrottle := errors.New("end first request after recording throttle")
		firstDone := make(chan error, 1)
		go func() {
			firstDone <- Run(ctx, first, fantasy.Call{}, Main, testSystemClock{}, func(Attempt) error { return stopAfterThrottle }, nil, nil)
		}()
		<-start

		var sent time.Time
		second := &policyModel{
			onCall:    func(int) { sent = time.Now() },
			responses: [][]fantasy.StreamPart{{textPart("ok"), finishPart()}},
		}
		secondDone := make(chan error, 1)
		go func() { secondDone <- Run(ctx, second, fantasy.Call{}, Main, testSystemClock{}, nil, nil, nil) }()
		synctest.Wait() // The second request has checked cooldown and is queued on the slot.
		allowRejection()
		<-blocked.classifying
		synctest.Wait()
		if second.calls != 0 || cooldowns.InFlight(key) != 1 {
			t.Fatalf("failing request released its slot before classification: calls=%d in-flight=%d", second.calls, cooldowns.InFlight(key))
		}

		until := time.Now().Add(45 * time.Second)
		allowClassification()
		if err := <-firstDone; !errors.Is(err, stopAfterThrottle) {
			t.Fatalf("first request did not observe the throttle: %v", err)
		}
		synctest.Wait()
		if second.calls != 0 || cooldowns.InFlight(key) != 0 {
			t.Fatalf("queued request bypassed cooldown or held a slot during it: calls=%d in-flight=%d", second.calls, cooldowns.InFlight(key))
		}
		if err := <-secondDone; err != nil {
			t.Fatalf("queued request failed after Retry-After: %v", err)
		}
		if second.calls != 1 || sent.Before(until) || cooldowns.InFlight(key) != 0 {
			t.Fatalf("queued request calls=%d sent=%v, want at or after %v with no leaked slot", second.calls, sent, until)
		}
	})
}

type testSystemClock struct{}

func (testSystemClock) Now() time.Time { return time.Now() }
func (testSystemClock) Sleep(ctx context.Context, d time.Duration) error {
	if d <= 0 {
		return ctx.Err()
	}
	timer := time.NewTimer(d)
	defer timer.Stop()
	select {
	case <-timer.C:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
