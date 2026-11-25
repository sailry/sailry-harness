package model

import (
	"context"
	"errors"
	"io"
	"net/http"
	"slices"
	"strings"
	"testing"

	"charm.land/fantasy"
)

func TestRunRequestObservation(t *testing.T) {
	t.Parallel()

	type reply struct {
		status     int
		failure    FailureKind
		retryAfter string
	}
	ok := reply{status: http.StatusOK}
	unauthorized := reply{status: http.StatusUnauthorized, failure: Authentication}
	busy := reply{status: http.StatusServiceUnavailable, failure: Unavailable}
	limited := reply{status: http.StatusTooManyRequests, failure: RateLimited}
	dial := reply{failure: Network}
	storeErr := errors.New("fixture attempt storage failed")

	tests := []struct {
		name          string
		script        []reply
		waits         int
		noObserver    bool
		cancelInWait  bool
		stopOnOutput  bool
		failStoreAt   int
		exhausted     bool
		waitExhausted bool
		want          RequestObservation
	}{
		{name: "first attempt success", script: []reply{ok}},
		{
			name: "first attempt permanent failure", script: []reply{unauthorized},
			want: RequestObservation{Outcome: RequestFailed, Attempts: 1, Failure: Authentication, Status: http.StatusUnauthorized},
		},
		{name: "permanent failure without observer", script: []reply{unauthorized}, noObserver: true},
		{
			name: "attempts exhausted", script: slices.Repeat([]reply{busy}, MainRetries+1), waits: MainRetries, exhausted: true,
			want: RequestObservation{Outcome: RequestFailed, Attempts: MainRetries + 1, Failure: Unavailable, Status: http.StatusServiceUnavailable},
		},
		{
			name: "wait exhausted", script: []reply{{status: http.StatusTooManyRequests, failure: RateLimited, retryAfter: "3600"}}, waitExhausted: true,
			want: RequestObservation{Outcome: RequestFailed, Attempts: 1, Failure: RateLimited, Status: http.StatusTooManyRequests},
		},
		{
			name: "different failures then success", script: []reply{busy, limited, ok}, waits: 2,
			want: RequestObservation{Outcome: RequestRecovered, Attempts: 3, Failure: RateLimited, Status: http.StatusTooManyRequests},
		},
		{
			name: "no stale status from an earlier failure", script: []reply{limited, dial, ok}, waits: 2,
			want: RequestObservation{Outcome: RequestRecovered, Attempts: 3, Failure: Network},
		},
		{
			name: "retry ends with a different permanent failure", script: []reply{busy, unauthorized}, waits: 1,
			want: RequestObservation{Outcome: RequestFailed, Attempts: 2, Failure: Authentication, Status: http.StatusUnauthorized},
		},
		{name: "cancel during retry wait", script: []reply{busy}, waits: 1, cancelInWait: true},
		{name: "cancel during replay", script: []reply{busy, {failure: Cancelled}}, waits: 1},
		{name: "consumer stops output", script: []reply{busy, {status: http.StatusOK, failure: Cancelled}}, waits: 1, stopOnOutput: true},
		{name: "cannot store failed attempt", script: []reply{busy}, failStoreAt: 1},
		{name: "cannot store recovered attempt", script: []reply{busy, ok}, waits: 1, failStoreAt: 2},
	}
	for _, provider := range []string{"responses", "plugin"} {
		success := "data: {\"kind\":\"text\",\"text\":\"Read complete\"}\n\ndata: {\"kind\":\"finish\",\"reason\":\"stop\"}\n\n"
		if provider == "responses" {
			success = string(fixture(t, "responses-text.sse"))
		}
		for _, test := range tests {
			t.Run(provider+"/"+test.name, func(t *testing.T) {
				var observations []RequestObservation
				var observe func(RequestObservation)
				if !test.noObserver {
					observe = func(result RequestObservation) { observations = append(observations, result) }
				}
				ctx := WithRequestObserver(t.Context(), observe)
				ctx, cancel := context.WithCancel(ctx)
				defer cancel()
				timer := &policyClock{}
				if test.cancelInWait {
					timer.cancel = cancel
				}
				requests := 0
				transport := pluginRoundTripper(func(request *http.Request) (*http.Response, error) {
					defer request.Body.Close()
					index := requests
					requests++
					if index >= len(test.script) {
						return nil, errors.New("unexpected extra request")
					}
					step := test.script[index]
					if step.status == 0 {
						if step.failure == Cancelled {
							cancel()
							return nil, request.Context().Err()
						}
						return nil, fixtureTCPDialTimeout()
					}
					body, mediaType := `{"error":{"message":"scripted provider failure"}}`, "application/json"
					if step.status == http.StatusOK {
						body, mediaType = success, "text/event-stream"
					}
					header := http.Header{"Content-Type": {mediaType}}
					if step.retryAfter != "" {
						header.Set("Retry-After", step.retryAfter)
					}
					return &http.Response{StatusCode: step.status, Header: header, Body: io.NopCloser(strings.NewReader(body))}, nil
				})
				language, call := requestObservationModel(t, provider, transport)
				var attempts []Attempt
				var output strings.Builder
				finished := false
				err := Run(ctx, language, call, Main, timer, func(attempt Attempt) error {
					if len(observations) != 0 {
						t.Fatalf("request observed before attempt %d was stored: %+v", attempt.Index, observations)
					}
					attempts = append(attempts, attempt)
					if len(attempts) == test.failStoreAt {
						return storeErr
					}
					return nil
				}, func(part fantasy.StreamPart) bool {
					if part.Type == fantasy.StreamPartTypeTextDelta {
						output.WriteString(part.Delta)
						if test.stopOnOutput {
							return false
						}
					}
					finished = finished || part.Type == fantasy.StreamPartTypeFinish
					return true
				}, nil)
				if requests != len(test.script) || len(attempts) != requests || len(timer.delays) != test.waits {
					t.Fatalf("requests=%d attempts=%d waits=%v, want %d requests/attempts and %d waits (err=%v)", requests, len(attempts), timer.delays, len(test.script), test.waits, err)
				}
				for index, attempt := range attempts {
					step := test.script[index]
					if attempt.RequestID == "" || attempt.RequestID != attempts[0].RequestID || attempt.Index != uint64(index) {
						t.Fatalf("attempt %d has a different request or index: %+v", index, attempt)
					}
					var kind FailureKind
					if attempt.Failure != nil {
						kind = attempt.Failure.Kind
						if attempt.Failure.Exhausted || attempt.Failure.WaitExhausted || attempt.Failure.RequestObserved() {
							t.Fatalf("attempt %d received terminal failure metadata: %+v", index, attempt.Failure)
						}
					}
					if kind != step.failure {
						t.Fatalf("attempt %d failure = %q, want %q", index, kind, step.failure)
					}
					if step.status == 0 && attempt.Status != nil || step.status != 0 && (attempt.Status == nil || int(*attempt.Status) != step.status) {
						t.Fatalf("attempt %d status = %v, want %d (0 means no response)", index, attempt.Status, step.status)
					}
				}
				wantFailure := test.script[len(test.script)-1].failure
				if test.cancelInWait {
					wantFailure = Cancelled
				}
				switch {
				case test.failStoreAt > 0:
					if !errors.Is(err, storeErr) {
						t.Fatalf("Run error = %v, want attempt storage error", err)
					}
				case wantFailure == "":
					if err != nil || !finished || output.String() != "Read complete" {
						t.Fatalf("Run error = %v, finish = %t, output = %q", err, finished, output.String())
					}
				default:
					failure, _ := err.(*Failure)
					if failure == nil || failure.Kind != wantFailure || failure.Exhausted != test.exhausted || failure.WaitExhausted != test.waitExhausted {
						t.Fatalf("Run failure = %+v, want %s (exhausted=%t, wait exhausted=%t)", failure, wantFailure, test.exhausted, test.waitExhausted)
					}
					wantObserved := test.want.Outcome == RequestFailed
					if got, classified := failure.RequestObserved(), Classify(err).RequestObserved(); got != wantObserved || classified != wantObserved {
						t.Fatalf("Run failure observed = %t (after classification: %t), want %t", got, classified, wantObserved)
					}
				}
				var want []RequestObservation
				if test.want.Outcome != "" {
					want = []RequestObservation{test.want}
				}
				if !slices.Equal(observations, want) {
					t.Fatalf("request observations = %+v, want %+v", observations, want)
				}
			})
		}
	}
}

func requestObservationModel(t *testing.T, provider string, transport pluginRoundTripper) (fantasy.LanguageModel, fantasy.Call) {
	t.Helper()
	if provider == "plugin" {
		const source = `export function prepare(input) { return {method:"POST",url:input.endpoint+"/messages",body:"{}"}; }
export function decode(input) { return {state:null,deltas:input.events.map(event=>JSON.parse(event.data))}; }`
		language, binding, _ := pluginFixture(t, source, transport)
		call, err := BuildCall(binding, Request{System: "fixture"})
		if err != nil {
			t.Fatal(err)
		}
		return language, call
	}
	language, err := newSDK(t.Context(), transportConfig{Kind: provider, Model: "fixture", Endpoint: "https://fixture.invalid", Key: "fixture-key"})
	if err != nil {
		t.Fatal(err)
	}
	language.(*sdkModel).client.Transport = responseTransport{base: transport, stall: stallTimeout}
	return language, requestCall(t, provider)
}
