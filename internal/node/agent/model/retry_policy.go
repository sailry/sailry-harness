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
	network, overloaded backoff.ExponentialBackOff
	waited              time.Duration
	dialTimeouts        uint64
	originDeadline      time.Time
	retryAt             time.Time
}

func newRetryPolicy() retryPolicy {
	policy := retryPolicy{
		network:    backoff.ExponentialBackOff{InitialInterval: time.Second, RandomizationFactor: 0.2, Multiplier: 2, MaxInterval: 30 * time.Second},
		overloaded: backoff.ExponentialBackOff{InitialInterval: 5 * time.Second, RandomizationFactor: 0.2, Multiplier: 2, MaxInterval: time.Minute},
	}
	policy.network.Reset()
	policy.overloaded.Reset()
	return policy
}

func (policy *retryPolicy) limit(purpose Purpose, attempt Attempt, streamed bool) uint64 {
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
	failure := attempt.Failure
	schedule := &policy.network
	if failure.Kind == RateLimited || failure.overloaded {
		schedule = &policy.overloaded
	}
	// The library caps the nominal interval, before jitter. Keep our actual
	// local wait within its cap; a provider's minimum may still exceed it.
	delay := min(schedule.NextBackOff(), schedule.MaxInterval)
	delay = max(delay, retryHint(failure, now))
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
