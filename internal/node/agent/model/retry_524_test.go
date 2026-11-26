package model

import (
	"bytes"
	"context"
	"io"
	"net/http"
	"testing"
	"testing/synctest"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/clock"
)

func TestHTTP524Recovery(t *testing.T) {
	t.Parallel()
	const latency = 2 * time.Minute
	const backoff = 30 * time.Second // The gateway's Retry-After.
	for _, test := range []struct {
		name           string
		recovers       bool
		latency        time.Duration
		deadline       time.Duration
		observe        time.Duration
		stalls         bool
		consume        time.Duration
		cancelAt       time.Duration
		requests       int
		waits          int
		elapsed        time.Duration
		transportStall time.Duration
		failureMessage string
	}{
		{name: "second request succeeds", recovers: true, latency: latency, requests: 2, waits: 1, elapsed: latency + backoff},
		{name: "persistent gateway timeout", latency: latency, requests: 2, waits: 1, elapsed: 2*latency + backoff},
		{name: "slow first request", latency: stallTimeout - time.Second, requests: 1, elapsed: stallTimeout - time.Second},
		{name: "retry stalls before request window ends", latency: latency, stalls: true, requests: 2, waits: 1, elapsed: latency + backoff + stallTimeout, failureMessage: "Model stream sent no data for 300 seconds"},
		{name: "retry stalls until request window ends", latency: latency, stalls: true, requests: 2, waits: 1, elapsed: retryWaitBudget, transportStall: retryWaitBudget, failureMessage: "Model retry timed out waiting for output after HTTP 524"},
		{name: "productive retry outlives window", latency: latency, recovers: true, consume: 8 * time.Minute, requests: 2, waits: 1, elapsed: latency + backoff + 8*time.Minute},
		{name: "upstream wait reaches deadline", latency: latency, deadline: latency + backoff, requests: 1, elapsed: latency},
		{name: "next gateway wait exceeds remaining runtime", latency: latency, deadline: 2*latency + backoff/2, requests: 1, elapsed: latency},
		{name: "persistence counts toward Retry-After", latency: latency, deadline: 2*latency + backoff + time.Second, observe: 2 * time.Second, recovers: true, requests: 2, waits: 1, elapsed: latency + backoff},
		{name: "runtime spent persisting retry", latency: latency, deadline: 2*latency + backoff + time.Second, observe: backoff + 2*time.Second, requests: 1, waits: 1, elapsed: latency + backoff + 2*time.Second},
		{name: "cancel during HTTP", latency: latency, cancelAt: latency / 2, requests: 1, elapsed: latency / 2},
		{name: "cancel during Retry-After", latency: latency, cancelAt: latency + backoff/2, requests: 1, waits: 1, elapsed: latency + backoff/2},
		{name: "cancel stalled replay", latency: latency, stalls: true, cancelAt: latency + backoff + time.Second, requests: 2, waits: 1, elapsed: latency + backoff + time.Second},
	} {
		t.Run(test.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				ctx, cancel := context.WithCancel(t.Context())
				defer cancel()
				if test.deadline > 0 {
					var stop context.CancelFunc
					ctx, stop = context.WithTimeout(ctx, test.deadline)
					defer stop()
				}
				if test.cancelAt > 0 {
					defer time.AfterFunc(test.cancelAt, cancel).Stop()
				}
				success := fixture(t, "responses-text.sse")
				requests := 0
				trip := pluginRoundTripper(func(req *http.Request) (*http.Response, error) {
					defer req.Body.Close()
					requests++
					if requests == 2 && test.stalls {
						<-req.Context().Done()
						return nil, req.Context().Err()
					}
					if requests == 2 && test.recovers {
						return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": {"text/event-stream"}}, Body: io.NopCloser(bytes.NewReader(success))}, nil
					}
					select {
					case <-req.Context().Done():
						return nil, req.Context().Err()
					case <-time.After(test.latency):
						return &http.Response{StatusCode: 524, Header: http.Header{"Retry-After": {"30"}, "Content-Type": {"text/html"}}, Body: io.NopCloser(bytes.NewBufferString("<html>Origin timed out</html>"))}, nil
					}
				})
				transportStall := stallTimeout
				if test.transportStall > 0 {
					transportStall = test.transportStall
				}
				language := &sdkModel{config: transportConfig{Kind: "responses", Model: "fixture", Endpoint: "https://fixture.invalid"}, client: &http.Client{Transport: responseTransport{base: trip, stall: transportStall}}}
				var attempts []Attempt
				finished, waits := false, 0
				started := time.Now()
				err := Run(ctx, language, requestCall(t, "responses"), Main, clock.System{}, func(a Attempt) error {
					attempts = append(attempts, a)
					if a.RetryDelay != nil {
						waits++
						time.Sleep(test.observe)
						if *a.RetryDelay != backoff {
							t.Fatalf("retry delay = %v, want %v", *a.RetryDelay, backoff)
						}
					}
					return nil
				}, func(part fantasy.StreamPart) bool {
					if part.Type == fantasy.StreamPartTypeTextDelta {
						time.Sleep(test.consume)
					}
					finished = finished || part.Type == fantasy.StreamPartTypeFinish
					return true
				}, nil)
				if elapsed := time.Since(started); requests != test.requests || len(attempts) != test.requests || waits != test.waits || elapsed != test.elapsed {
					t.Fatalf("requests=%d observations=%d waits=%d elapsed=%v, want %d requests, %d waits and %v", requests, len(attempts), waits, elapsed, test.requests, test.waits, test.elapsed)
				}
				if finished != test.recovers || (err == nil) != test.recovers {
					t.Fatalf("finished=%t error=%v", finished, err)
				}
				failure := Classify(err)
				if test.failureMessage != "" && (failure == nil || failure.Message != test.failureMessage) {
					t.Fatalf("failure = %+v, want message %q", failure, test.failureMessage)
				}
				if test.cancelAt > 0 {
					if failure == nil || failure.Kind != Cancelled || failure.Exhausted || failure.WaitExhausted {
						t.Fatalf("cancellation lost to retry policy: %+v", failure)
					}
				} else if test.stalls {
					if failure.Kind != Network || !failure.WaitExhausted || failure.Exhausted || attempts[1].Status != nil {
						t.Fatalf("stalled retry lost its wait limit: %+v, status=%v", failure, attempts[1].Status)
					}
				} else if !test.recovers {
					if failure.Kind != Unavailable || failure.status == nil || *failure.status != 524 || failure.Exhausted != (requests == 2) || failure.WaitExhausted != (requests == 1) {
						t.Fatalf("gateway failure lost its cause or recovery limit: %+v", failure)
					}
				}
				if test.cancelAt == 0 || test.cancelAt > test.latency {
					first := attempts[0]
					if first.Status == nil || *first.Status != 524 || first.Failure == nil || first.Failure.Kind != Unavailable ||
						first.ResponseUs == nil || *first.ResponseUs != uint64(test.latency.Microseconds()) || first.Finished.Sub(first.Started) != test.latency {
						t.Fatalf("incorrect HTTP status, classification or attempt timing: %+v", first)
					}
				}
			})
		})
	}
}

func TestHTTP524KeepsOtherTransientRetries(t *testing.T) {
	t.Parallel()
	for _, purpose := range []Purpose{Main, SummaryPurpose} {
		t.Run(string(purpose), func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				gateway := &fantasy.ProviderError{StatusCode: 524, Message: "origin timed out"}
				busy := &fantasy.ProviderError{StatusCode: 429, ResponseHeaders: map[string]string{"Retry-After": "37"}}
				fake := &policyModel{startErrors: []error{gateway, busy, gateway}}
				var attempts []Attempt
				err := Run(t.Context(), fake, fantasy.Call{}, purpose, clock.System{}, func(a Attempt) error {
					attempts = append(attempts, a)
					return nil
				}, nil, nil)
				failure := Classify(err)
				if fake.calls != 3 || len(attempts) != 3 || attempts[2].Started.Sub(attempts[1].Finished) != 37*time.Second || failure == nil || !failure.Exhausted || failure.status == nil || *failure.status != 524 {
					t.Fatalf("mixed failures lost their retry limits: calls=%d attempts=%+v failure=%+v", fake.calls, attempts, failure)
				}
			})
		})
	}
}

func TestHTTP524AfterFinishDoesNotReplay(t *testing.T) {
	t.Parallel()
	failure := fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &fantasy.ProviderError{StatusCode: 524, Message: "origin timed out"}}
	fake := &policyModel{responses: [][]fantasy.StreamPart{{textPart("answer"), finishPart(), failure}}}
	commits, resets := 0, 0
	timer := &policyClock{}
	err := Run(t.Context(), fake, fantasy.Call{}, Main, timer, nil, func(part fantasy.StreamPart) bool {
		if part.Type == fantasy.StreamPartTypeFinish {
			commits++
		}
		return true
	}, func() { resets++ })
	if f := Classify(err); f == nil || f.status == nil || *f.status != 524 || f.Exhausted || f.WaitExhausted || fake.calls != 1 || commits != 1 || resets != 0 || len(timer.delays) != 0 {
		t.Fatalf("tail failure replayed committed output: calls=%d commits=%d resets=%d delays=%v failure=%+v", fake.calls, commits, resets, timer.delays, f)
	}
}

func TestStreamStallMeasuresOnlyPendingIO(t *testing.T) {
	t.Parallel()
	synctest.Test(t, func(t *testing.T) {
		trip := pluginRoundTripper(func(req *http.Request) (*http.Response, error) {
			reader, writer := io.Pipe()
			// Headers arrive near the end of their own idle timeout.
			time.Sleep(stallTimeout - time.Second)
			go func() {
				defer writer.Close()
				time.Sleep(2 * time.Second)
				_, _ = writer.Write([]byte("one"))
				_, _ = writer.Write([]byte("two"))
			}()
			context.AfterFunc(req.Context(), func() { reader.CloseWithError(req.Context().Err()) })
			return &http.Response{StatusCode: http.StatusOK, Header: make(http.Header), Body: reader}, nil
		})
		req, err := http.NewRequestWithContext(t.Context(), http.MethodGet, "https://fixture.invalid", nil)
		if err != nil {
			t.Fatal(err)
		}
		transport := responseTransport{base: trip, stall: stallTimeout}
		resp, err := transport.RoundTrip(req)
		if err != nil {
			t.Fatal(err)
		}
		defer resp.Body.Close()
		var data [3]byte
		for _, want := range []string{"one", "two"} {
			if _, err := io.ReadFull(resp.Body, data[:]); err != nil || string(data[:]) != want {
				t.Fatalf("expected %q after an active read, got %q, %v", want, data, err)
			}
			// The model consumer is publishing output, not waiting for bytes.
			time.Sleep(stallTimeout + time.Second)
		}
	})
}

func TestSilentStreamStallSharesRetryWaitBudget(t *testing.T) {
	t.Parallel()
	synctest.Test(t, func(t *testing.T) {
		transport := responseTransport{stall: stallTimeout, base: pluginRoundTripper(func(req *http.Request) (*http.Response, error) {
			defer req.Body.Close()
			<-req.Context().Done()
			return nil, req.Context().Err()
		})}
		language := &sdkModel{config: transportConfig{Kind: "responses", Model: "fixture", Endpoint: "https://fixture.invalid"}, client: &http.Client{Transport: transport}}
		var attempts []Attempt
		started := time.Now()
		err := Run(t.Context(), language, requestCall(t, "responses"), Main, clock.System{}, func(a Attempt) error {
			attempts = append(attempts, a)
			return nil
		}, nil, nil)
		failure := Classify(err)
		if failure == nil || failure.Kind != Network || !failure.WaitExhausted || failure.Exhausted || len(attempts) != 2 {
			t.Fatalf("attempts=%d failure=%+v", len(attempts), failure)
		}
		if delay := attempts[0].RetryDelay; delay == nil || attempts[1].RetryDelay != nil || time.Since(started) != 2*stallTimeout+*delay {
			t.Fatalf("silent waits=%v elapsed=%s", attempts, time.Since(started))
		}
	})
}

func TestStallAfterModelOutputKeepsNormalRetry(t *testing.T) {
	t.Parallel()
	for _, part := range []fantasy.StreamPart{textPart("partial"), {Type: fantasy.StreamPartTypeToolInputStart, ToolCallName: "read"}} {
		t.Run(string(part.Type), func(t *testing.T) {
			stalled := []fantasy.StreamPart{part, {Type: fantasy.StreamPartTypeError, Error: stallError{idle: stallTimeout}}}
			model := &policyModel{responses: [][]fantasy.StreamPart{stalled, stalled, {finishPart()}}}
			if err := Run(t.Context(), model, fantasy.Call{}, Main, &policyClock{}, nil, nil, nil); err != nil || model.calls != 3 {
				t.Fatalf("attempts=%d error=%v", model.calls, err)
			}
		})
	}
}
