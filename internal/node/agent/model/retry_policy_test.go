package model

import (
	"fmt"
	"net/http"
	"testing"
	"time"

	"charm.land/fantasy"
)

func TestRetryWaitBudgetIsSharedAcrossFailures(t *testing.T) {
	for _, purpose := range []Purpose{Main, SummaryPurpose} {
		for _, waits := range [][]int{{240, 240, 121}, {300, 300, 1}} {
			t.Run(fmt.Sprintf("%s/%v", purpose, waits), func(t *testing.T) {
				fake := &policyModel{}
				for index, seconds := range waits {
					status := 502
					if index == 1 {
						status = 429
					}
					fake.responses = append(fake.responses, []fantasy.StreamPart{{
						Type: fantasy.StreamPartTypeError,
						Error: &fantasy.ProviderError{StatusCode: status, Message: "busy",
							ResponseHeaders: map[string]string{"Retry-After": fmt.Sprint(seconds)}},
					}})
				}
				timer := &policyClock{}
				var attempts []Attempt
				err := Run(t.Context(), fake, fantasy.Call{}, purpose, timer, func(attempt Attempt) error {
					attempts = append(attempts, attempt)
					return nil
				}, nil)
				failure := Classify(err)
				if failure == nil || !failure.WaitExhausted || failure.Exhausted || failure.Kind != Unavailable {
					t.Fatalf("the shared wait budget lost its cause: %+v", failure)
				}
				if fake.calls != 3 || len(timer.delays) != 2 || len(attempts) != 3 || attempts[2].RetryDelay != nil {
					t.Fatalf("a wait beyond the budget was shortened or retried: calls=%d waits=%v attempts=%+v", fake.calls, timer.delays, attempts)
				}
				for index, delay := range timer.delays {
					if delay != time.Duration(waits[index])*time.Second || attempts[index].RetryDelay == nil || *attempts[index].RetryDelay != delay {
						t.Fatalf("provider minimum above the local cap changed: waits=%v", timer.delays)
					}
				}
			})
		}
	}
}

func TestRetryHintMinimum(t *testing.T) {
	now := time.Date(2037, time.October, 21, 7, 28, 0, 0, time.UTC)
	for _, sample := range []struct {
		name    string
		headers []string
		millis  *uint64
		want    time.Duration
	}{
		{"all sources", []string{" 4 ", now.Add(11 * time.Second).Format(http.TimeFormat)}, retryMilliseconds(9000), 11 * time.Second},
		{"adapter is longest", []string{"4", now.Add(11 * time.Second).Format(http.TimeFormat)}, retryMilliseconds(12000), 12 * time.Second},
		{"ignore invalid and past", []string{"", "-3", "1.5", "invalid", now.Add(-time.Hour).Format(http.TimeFormat)}, nil, 0},
		{"overflow seconds", []string{"18446744073709551616"}, nil, time.Duration(1<<63 - 1)},
		{"overflow duration", []string{"18446744073709551615"}, nil, time.Duration(1<<63 - 1)},
		{"overflow milliseconds", nil, retryMilliseconds(^uint64(0)), time.Duration(1<<63 - 1)},
	} {
		t.Run(sample.name, func(t *testing.T) {
			failure := &Failure{Kind: Unavailable, retryAfter: sample.headers, retryAfterMs: sample.millis}
			if got := retryHint(failure, now); got != sample.want {
				t.Fatalf("retry minimum=%v, want %v", got, sample.want)
			}
		})
	}
}

func retryMilliseconds(value uint64) *uint64 { return &value }
