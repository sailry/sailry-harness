package model

import (
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/cenkalti/backoff/v5"
)

const retryWaitBudget = 10 * time.Minute

type retryPolicy struct {
	network, overloaded backoff.ExponentialBackOff
	waited              time.Duration
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

func (policy *retryPolicy) next(failure *Failure, now time.Time) (time.Duration, bool) {
	schedule := &policy.network
	if failure.Kind == RateLimited || failure.overloaded {
		schedule = &policy.overloaded
	}
	// The library caps the nominal interval, before jitter. Keep our actual
	// local wait within its cap; a provider's minimum may still exceed it.
	delay := min(schedule.NextBackOff(), schedule.MaxInterval)
	delay = max(delay, retryHint(failure, now))
	// Never shorten the provider's minimum to fit the remaining budget.
	return delay, delay <= retryWaitBudget-policy.waited
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
