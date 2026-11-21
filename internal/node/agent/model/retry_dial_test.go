package model

import (
	"context"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"strings"
	"syscall"
	"testing"
	"time"

	"charm.land/fantasy"
)

func fixtureTCPDialTimeout() *net.OpError {
	return &net.OpError{Op: "dial", Net: "tcp", Addr: &net.TCPAddr{Port: 443}, Err: os.ErrDeadlineExceeded}
}

func TestMainTCPDialTimeoutStopsAfterThreeAttempts(t *testing.T) {
	for _, provider := range []string{"responses", "chat", "anthropic", "google", "plugin"} {
		t.Run(provider, func(t *testing.T) {
			timeout := fixtureTCPDialTimeout()
			requests := 0
			transport := pluginRoundTripper(func(request *http.Request) (*http.Response, error) {
				requests++
				if request.Body != nil {
					request.Body.Close()
				}
				return nil, timeout
			})
			var language fantasy.LanguageModel
			var call fantasy.Call
			var err error
			diagnostic := timeout.Error()
			if provider == "plugin" {
				plugin, binding, _ := pluginFixture(t, `export function prepare(input) { return {method:"POST",url:input.endpoint,body:"{}"}; }`, transport)
				language = plugin
				call, err = BuildCall(binding, Request{System: "fixture"})
				diagnostic = "model connection failed"
			} else {
				language, err = newSDK(t.Context(), transportConfig{Kind: provider, Model: "fixture", Endpoint: "https://fixture.invalid", Key: "fixture-key"})
				if err != nil {
					t.Fatal(err)
				}
				language.(*sdkModel).client.Transport = responseTransport{base: transport, stall: stallTimeout}
				call = requestCall(t, provider)
			}
			if err != nil {
				t.Fatal(err)
			}
			timer := &policyClock{}
			var attempts []Attempt
			err = Run(t.Context(), language, call, Main, timer, func(attempt Attempt) error {
				attempts = append(attempts, attempt)
				return nil
			}, nil, nil)
			failure := Classify(err)
			if requests != 3 || len(timer.delays) != 2 || len(attempts) != 3 || failure == nil || failure.Kind != Network || !failure.Exhausted || failure.WaitExhausted || !strings.Contains(failure.Message, diagnostic) {
				t.Fatalf("TCP timeout recovery: requests=%d, waits=%v, attempts=%d, failure=%+v", requests, timer.delays, len(attempts), failure)
			}
			for index, attempt := range attempts {
				if attempt.Index != uint64(index) || attempt.RetryLimit != 2 || attempt.Failure == nil || attempt.Failure.Exhausted || attempt.Status != nil || attempt.RequestSentUs != nil {
					t.Fatalf("wrong attempt facts: %+v", attempt)
				}
				if index < len(timer.delays) {
					if attempt.RetryDelay == nil || *attempt.RetryDelay != timer.delays[index] {
						t.Fatalf("wrong announced wait for attempt %d: %v", index, attempt.RetryDelay)
					}
				} else if attempt.RetryDelay != nil {
					t.Fatalf("terminal attempt announced a retry: %+v", attempt)
				}
			}
		})
	}
}

func TestMainTCPDialTimeoutStreakResets(t *testing.T) {
	dial := fixtureTCPDialTimeout()
	throttled := &fantasy.ProviderError{StatusCode: http.StatusTooManyRequests, ResponseHeaders: map[string]string{"Retry-After": "40"}}
	fake := &policyModel{startErrors: []error{dial, dial, throttled, dial, dial}, responses: make([][]fantasy.StreamPart, 6)}
	fake.responses[5] = []fantasy.StreamPart{textPart("recovered"), finishPart()}
	timer := &policyClock{}
	var output string
	var attempts []Attempt
	err := Run(t.Context(), fake, fantasy.Call{}, Main, timer, func(attempt Attempt) error {
		attempts = append(attempts, attempt)
		return nil
	}, func(part fantasy.StreamPart) bool {
		output += part.Delta
		return true
	}, nil)
	if err != nil || output != "recovered" || fake.calls != 6 || len(attempts) != 6 || len(timer.delays) != 5 || timer.delays[2] != 40*time.Second {
		t.Fatalf("mixed recovery: calls=%d, waits=%v, attempts=%d, output=%q, error=%v", fake.calls, timer.delays, len(attempts), output, err)
	}
	for index, want := range []uint64{2, 2, MainRetries, 5, 5, MainRetries} {
		if got := attempts[index].RetryLimit; got != want {
			t.Errorf("attempt %d recorded limit %d, want %d", index+1, got, want)
		}
	}
}

func TestTCPDialTimeoutScope(t *testing.T) {
	dial := fixtureTCPDialTimeout()
	for _, test := range []struct {
		name    string
		purpose Purpose
		err     error
		partial bool
	}{
		{name: "summary", purpose: SummaryPurpose, err: dial},
		{name: "DNS", purpose: Main, err: &net.OpError{Op: "dial", Net: "tcp", Err: &net.DNSError{IsTimeout: true}}},
		{name: "refused", purpose: Main, err: &net.OpError{Op: "dial", Net: "tcp", Addr: dial.Addr, Err: syscall.ECONNREFUSED}},
		{name: "TLS", purpose: Main, err: &url.Error{Op: "Post", URL: "https://fixture.invalid", Err: fmt.Errorf("TLS handshake: %w", context.DeadlineExceeded)}},
		{name: "read", purpose: Main, err: &net.OpError{Op: "read", Net: "tcp", Addr: dial.Addr, Err: dial.Err}},
		{name: "truncation", purpose: Main, err: io.ErrUnexpectedEOF, partial: true},
		{name: "after response", purpose: Main, err: &fantasy.ProviderError{StatusCode: http.StatusOK, Cause: dial}},
		{name: "after output", purpose: Main, err: dial, partial: true},
		{name: "diagnostic text", purpose: Main, err: &Failure{Kind: Network, Message: dial.Error()}},
	} {
		t.Run(test.name, func(t *testing.T) {
			parts := []fantasy.StreamPart{{Type: fantasy.StreamPartTypeError, Error: test.err}}
			if test.partial {
				parts = append([]fantasy.StreamPart{textPart("partial")}, parts...)
			}
			fake := &policyModel{}
			for range MainRetries + 1 {
				fake.responses = append(fake.responses, parts)
			}
			timer := &policyClock{}
			err := Run(t.Context(), fake, fantasy.Call{}, test.purpose, timer, func(attempt Attempt) error {
				if attempt.RetryLimit != MainRetries {
					t.Errorf("attempt %d recorded limit %d", attempt.Index+1, attempt.RetryLimit)
				}
				return nil
			}, func(fantasy.StreamPart) bool { return true }, func() {})
			failure := Classify(err)
			if fake.calls != MainRetries+1 || len(timer.delays) != MainRetries || failure == nil || !failure.Exhausted || failure.WaitExhausted {
				t.Fatalf("generic retry budget: attempts=%d, waits=%v, failure=%+v", fake.calls, timer.delays, failure)
			}
		})
	}
}

func TestTCPDialTimeoutHonorsCancellation(t *testing.T) {
	for _, wait := range []bool{true, false} {
		name := "on third dial"
		if wait {
			name = "during retry wait"
		}
		t.Run(name, func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			dial := fixtureTCPDialTimeout()
			fake := &policyModel{startErrors: []error{dial, dial, dial}}
			timer := &policyClock{}
			wantCalls, wantWaits := 3, 2
			if wait {
				timer.cancel = cancel
				wantCalls, wantWaits = 1, 1
			} else {
				fake.onCall = func(index int) {
					if index == 2 {
						cancel()
					}
				}
			}
			err := Run(ctx, fake, fantasy.Call{}, Main, timer, nil, nil, nil)
			failure := Classify(err)
			if failure == nil || failure.Kind != Cancelled || failure.Exhausted || failure.WaitExhausted || fake.calls != wantCalls || len(timer.delays) != wantWaits {
				t.Fatalf("cancellation: calls=%d, waits=%v, failure=%+v", fake.calls, timer.delays, failure)
			}
		})
	}
}
