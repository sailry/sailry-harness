package model

import (
	"context"
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/cenkalti/backoff/v5"
	"github.com/sailry/sailry-harness/internal/node/agent/clock"
)

const (
	retryWaitBudget            = 10 * time.Minute
	mainTCPDialTimeoutAttempts = 3
	statusOriginTimeout        = 524
)

type retryPolicy struct {
	schedule       backoff.ExponentialBackOff
	waited         time.Duration
	dialTimeouts   uint64
	httpRetried    bool
	originDeadline time.Time
	retryAt        time.Time
	// nativeResends counts the provider errors of a native compaction, which
	// share its quick resend whatever waits came between them.
	nativeResends uint64
}

// nativeRetries is the quick resend a native compaction gets for a provider
// error before Sailry's own summary replaces it. A rate limit, overload, spent
// balance or refused account waits as other requests do, since the summary
// would meet it too.
const nativeRetries = 1

func newRetryPolicy() retryPolicy {
	// Most transient failures clear at once, so the first resend is quick.
	// Rate limits and overload share the schedule: a provider that needs a
	// slower pace says so through its wait hint.
	policy := retryPolicy{
		schedule: backoff.ExponentialBackOff{InitialInterval: 300 * time.Millisecond, RandomizationFactor: 0.2, Multiplier: 2, MaxInterval: 30 * time.Second},
	}
	policy.schedule.Reset()
	return policy
}

// A received HTTP rejection gets one resend even when its diagnosis is not
// transient. It shares the request's budgets, not its exponential schedule.
func (policy *retryPolicy) takeHTTPRetry(attempt Attempt) bool {
	if policy.httpRetried || attempt.Index >= MainRetries || attempt.Failure.Kind == Cancelled ||
		attempt.Status == nil || *attempt.Status < 400 || *attempt.Status >= 600 {
		return false
	}
	policy.httpRetried = true
	policy.dialTimeouts = 0
	return true
}

func (policy *retryPolicy) limit(purpose Purpose, attempt Attempt, streamed bool) uint64 {
	if purpose == NativePurpose && attempt.Failure.nativeFallback() {
		policy.nativeResends++
		return min(MainRetries, attempt.Index+nativeRetries+1-policy.nativeResends)
	}
	if purpose == Main && attempt.Failure.tcpDialTimeout && !streamed && attempt.RequestSentUs == nil && attempt.Status == nil {
		policy.dialTimeouts++
		return min(MainRetries, attempt.Index+mainTCPDialTimeoutAttempts-policy.dialTimeouts)
	}
	policy.dialTimeouts = 0
	if attempt.Status != nil && *attempt.Status == statusOriginTimeout {
		if !policy.originDeadline.IsZero() {
			return attempt.Index
		}
		policy.originDeadline = attempt.RequestStarted.Add(retryWaitBudget)
		return min(MainRetries, attempt.Index+1)
	}
	return MainRetries
}

func (policy *retryPolicy) fits(ctx context.Context, attempt Attempt, now time.Time, delay time.Duration) bool {
	deadline, bounded := ctx.Deadline()
	if policy.originDeadline.IsZero() {
		return !bounded || delay < deadline.Sub(now)
	}
	// An origin timeout has already consumed wall time. Later failures on the
	// same request share the remaining window, even if the HTTP status changes.
	remaining := policy.originDeadline.Sub(now)
	if bounded {
		remaining = min(remaining, deadline.Sub(now))
	}
	return delay < remaining && attempt.Finished.Sub(attempt.Started) < remaining-delay
}

// An admitted retry after a 524 has the remaining request window to start
// producing output. A productive stream keeps its ordinary idle timeout.
func (policy *retryPolicy) firstOutputTimer(start time.Time, cancel context.CancelFunc) *time.Timer {
	if policy.originDeadline.IsZero() {
		return nil
	}
	return time.AfterFunc(policy.originDeadline.Sub(start), cancel)
}

func (policy *retryPolicy) next(ctx context.Context, attempt Attempt, now time.Time) (time.Duration, bool) {
	delay := policy.schedule.InitialInterval
	if attempt.Failure.Retryable() {
		// The library caps the nominal interval, before jitter. Keep our actual
		// local wait within its cap; a provider's minimum may still exceed it.
		delay = min(policy.schedule.NextBackOff(), policy.schedule.MaxInterval)
	}
	delay = max(delay, retryHint(attempt.Failure, now))
	if !policy.originDeadline.IsZero() {
		policy.retryAt = now.Add(delay)
	}
	// Never shorten the provider's minimum to fit the remaining budget.
	return delay, delay <= retryWaitBudget-policy.waited && policy.fits(ctx, attempt, now, delay)
}

func (policy *retryPolicy) wait(ctx context.Context, timer clock.Clock, attempt Attempt) (bool, error) {
	delay := *attempt.RetryDelay
	// Persisting the retry also lets time pass toward the gateway's minimum.
	now := timer.Now()
	if !policy.retryAt.IsZero() {
		delay = max(0, policy.retryAt.Sub(now))
	}
	if !policy.fits(ctx, attempt, now, delay) {
		return false, ctx.Err()
	}
	if err := timer.Sleep(ctx, delay); err != nil {
		return false, err
	}
	if err := ctx.Err(); err != nil {
		return false, err
	}
	policy.waited += delay
	return policy.fits(ctx, attempt, timer.Now(), 0), nil
}

// waitCooldown waits out an active credential-wide rate-limit cooldown before
// sending an attempt so concurrent requests sharing the same credential do not
// bypass an in-progress Retry-After window.
func (policy *retryPolicy) waitCooldown(ctx context.Context, timer clock.Clock, cooldowns *Cooldowns, key string) (bool, error) {
	if cooldowns == nil || key == "" {
		return true, nil
	}
	var lastUntil time.Time
	for {
		now := timer.Now()
		var until time.Time
		var stagger time.Duration
		var active bool
		if lastUntil.IsZero() {
			until, stagger, active = cooldowns.WaitWindow(key, now)
		} else {
			until, active = cooldowns.Until(key, now)
			if active && !until.Equal(lastUntil) {
				until, stagger, active = cooldowns.WaitWindow(key, now)
			}
		}
		if !active || !until.After(now) {
			return true, nil
		}
		lastUntil = until
		base := until.Sub(now)
		deadline, bounded := ctx.Deadline()
		if base > retryWaitBudget-policy.waited || bounded && base >= deadline.Sub(now) {
			return false, nil
		}
		if !policy.originDeadline.IsZero() && base >= policy.originDeadline.Sub(now) {
			return false, nil
		}
		headroom := retryWaitBudget - policy.waited - base
		if bounded {
			headroom = min(headroom, deadline.Sub(now)-base-time.Nanosecond)
		}
		if !policy.originDeadline.IsZero() {
			headroom = min(headroom, policy.originDeadline.Sub(now)-base-time.Nanosecond)
		}
		delay := base + max(0, min(stagger, headroom))
		if err := timer.Sleep(ctx, delay); err != nil {
			return false, err
		}
		if err := ctx.Err(); err != nil {
			return false, err
		}
		policy.waited += delay
	}
}

func retryHint(failure *Failure, now time.Time) time.Duration {
	var delay time.Duration
	for _, raw := range failure.retryAfter {
		value := strings.TrimSpace(raw)
		seconds, err := strconv.ParseUint(value, 10, 64)
		switch {
		case err == nil:
			delay = max(delay, retryHintDuration(seconds, time.Second))
		case errors.Is(err, strconv.ErrRange):
			delay = time.Duration(1<<63 - 1)
		default:
			if date, err := http.ParseTime(value); err == nil {
				delay = max(delay, date.Sub(now))
			}
		}
	}
	if failure.retryAfterMs != nil {
		delay = max(delay, retryHintDuration(*failure.retryAfterMs, time.Millisecond))
	}
	return delay
}

func retryHintDuration(value uint64, unit time.Duration) time.Duration {
	const limit = time.Duration(1<<63 - 1)
	if value > uint64(limit/unit) {
		return limit
	}
	return time.Duration(value) * unit
}
