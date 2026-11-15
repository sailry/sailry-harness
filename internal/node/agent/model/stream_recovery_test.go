package model

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"reflect"
	"testing"

	"charm.land/fantasy"
)

func TestStreamingRetryResetsOnlyTheFailedAttempt(t *testing.T) {
	for _, event := range []fantasy.StreamPart{
		textPart("discard"),
		{Type: fantasy.StreamPartTypeReasoningDelta, ID: "thought", Delta: "discard"},
		{Type: fantasy.StreamPartTypeToolInputDelta, ID: "call", Delta: `{"path":`},
		{Type: fantasy.StreamPartTypeToolCall, ID: "call", ToolCallName: "read", ToolCallInput: `{}`},
		{Type: fantasy.StreamPartTypeKeepalive},
	} {
		for _, cause := range []error{io.ErrUnexpectedEOF, &Failure{Kind: Network}, &Failure{Kind: RateLimited}, &Failure{Kind: Unavailable}} {
			t.Run(string(event.Type)+"/"+string(Classify(cause).Kind), func(t *testing.T) {
				fake := &policyModel{responses: [][]fantasy.StreamPart{{event, {Type: fantasy.StreamPartTypeError, Error: cause}}, {textPart("keep"), finishPart()}}}
				var output []fantasy.StreamPart
				resets := 0
				timer := &policyClock{beforeSleep: func() {
					if resets != 1 || len(output) != 0 {
						t.Fatal("retry waited before releasing provisional output")
					}
				}}
				var attempts []Attempt
				err := Run(t.Context(), fake, fantasy.Call{}, Main, timer, func(attempt Attempt) error {
					attempts = append(attempts, attempt)
					return nil
				}, func(part fantasy.StreamPart) bool {
					output = append(output, part)
					return true
				}, func() { output = nil; resets++ })
				if err != nil || fake.calls != 2 || resets != 1 || !reflect.DeepEqual(output, []fantasy.StreamPart{textPart("keep"), finishPart()}) {
					t.Fatalf("recovered output=%+v calls=%d resets=%d error=%v", output, fake.calls, resets, err)
				}
				if len(attempts) != 2 || attempts[0].RetryDelay == nil || attempts[1].Failure != nil || attempts[0].RequestID != attempts[1].RequestID {
					t.Fatalf("recovery lost its attempts or request identity: %+v", attempts)
				}
			})
		}
	}
	t.Run("missing terminal event", func(t *testing.T) {
		fake := &policyModel{responses: [][]fantasy.StreamPart{{textPart("discard")}, {textPart("keep"), finishPart()}}}
		var output string
		err := Run(t.Context(), fake, fantasy.Call{}, Main, &policyClock{}, nil, func(part fantasy.StreamPart) bool {
			output += part.Delta
			return true
		}, func() { output = "" })
		if err != nil || fake.calls != 2 || output != "keep" {
			t.Fatalf("missing terminal event recovery: calls=%d output=%q error=%v", fake.calls, output, err)
		}
	})
	t.Run("keepalive without a reset owner", func(t *testing.T) {
		fake := &policyModel{responses: [][]fantasy.StreamPart{{{Type: fantasy.StreamPartTypeKeepalive}, errorPart(Network)}, {finishPart()}}}
		if err := Run(t.Context(), fake, fantasy.Call{}, Main, &policyClock{}, nil, func(fantasy.StreamPart) bool { return true }, nil); err != nil || fake.calls != 2 {
			t.Fatalf("keepalive prevented a safe retry: calls=%d error=%v", fake.calls, err)
		}
	})
}

func TestStreamingRetryPreservesTerminalOutputAndCancellation(t *testing.T) {
	t.Run("attempt limit", func(t *testing.T) {
		fake := &policyModel{}
		for index := range MainRetries + 1 {
			fake.responses = append(fake.responses, []fantasy.StreamPart{textPart(fmt.Sprint(index)), {Type: fantasy.StreamPartTypeError, Error: io.ErrUnexpectedEOF}})
		}
		output, resets := "", 0
		err := Run(t.Context(), fake, fantasy.Call{}, Main, &policyClock{}, nil, func(part fantasy.StreamPart) bool { output += part.Delta; return true }, func() { output = ""; resets++ })
		if failure := Classify(err); failure == nil || !failure.Exhausted || failure.Kind != InvalidResponse || fake.calls != MainRetries+1 || resets != MainRetries || output != fmt.Sprint(MainRetries) {
			t.Fatalf("terminal output=%q calls=%d resets=%d failure=%+v", output, fake.calls, resets, failure)
		}
	})
	for _, release := range []bool{false, true} {
		t.Run(fmt.Sprintf("cancellation/release_%t", release), func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			fake := &policyModel{responses: [][]fantasy.StreamPart{{textPart("retain"), {Type: fantasy.StreamPartTypeError, Error: io.ErrUnexpectedEOF}}}}
			resets := 0
			timer := &policyClock{}
			err := Run(ctx, fake, fantasy.Call{}, Main, timer, nil, func(fantasy.StreamPart) bool {
				if !release {
					cancel()
				}
				return !release
			}, func() { resets++ })
			if failure := Classify(err); failure == nil || failure.Kind != Cancelled || fake.calls != 1 || resets != 0 || len(timer.delays) != 0 {
				t.Fatalf("cancellation restarted the request: calls=%d resets=%d failure=%+v", fake.calls, resets, failure)
			}
		})
	}
	t.Run("cancellation during retry observation", func(t *testing.T) {
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		fake := &policyModel{responses: [][]fantasy.StreamPart{{textPart("provisional"), {Type: fantasy.StreamPartTypeError, Error: io.ErrUnexpectedEOF}}}}
		resets := 0
		timer := &policyClock{}
		err := Run(ctx, fake, fantasy.Call{}, Main, timer, func(attempt Attempt) error {
			if attempt.RetryDelay != nil {
				cancel()
			}
			return nil
		}, func(fantasy.StreamPart) bool { return true }, func() { resets++ })
		if failure := Classify(err); failure == nil || failure.Kind != Cancelled || fake.calls != 1 || resets != 1 || len(timer.delays) != 0 {
			t.Fatalf("committed retry left its old output live after cancellation: calls=%d resets=%d failure=%+v", fake.calls, resets, failure)
		}
	})
}

func TestTruncatedResponseIsDistinctFromMalformedOrRejectedResponse(t *testing.T) {
	var object any
	syntax := json.Unmarshal([]byte(`{broken`), &object)
	for _, cause := range []error{
		syntax,
		&Failure{Kind: InvalidResponse, Message: "malformed response"},
		&fantasy.ProviderError{StatusCode: 401, Cause: io.ErrUnexpectedEOF},
		&fantasy.ProviderError{StatusCode: 400, Cause: io.ErrUnexpectedEOF},
	} {
		t.Run(cause.Error(), func(t *testing.T) {
			if failure := Classify(cause); failure.Retryable() {
				t.Fatalf("malformed or rejected response became retryable: %+v", failure)
			}
			fake := &policyModel{responses: [][]fantasy.StreamPart{{textPart("retain"), {Type: fantasy.StreamPartTypeError, Error: cause}}}}
			resets := 0
			err := Run(t.Context(), fake, fantasy.Call{}, Main, &policyClock{}, nil, func(fantasy.StreamPart) bool { return true }, func() { resets++ })
			if err == nil || fake.calls != 1 || resets != 0 {
				t.Fatalf("permanent failure restarted the request: calls=%d resets=%d error=%v", fake.calls, resets, err)
			}
		})
	}
	if !Classify(Classify(io.ErrUnexpectedEOF)).Retryable() || !Classify(fmt.Errorf("wrapped: %w", io.ErrUnexpectedEOF)).Retryable() || !Classify(&fantasy.ProviderError{StatusCode: 200, Cause: io.ErrUnexpectedEOF}).Retryable() {
		t.Fatal("a transport EOF lost its recoverable classification")
	}
	if Classify(errors.New("unexpected EOF")).Retryable() {
		t.Fatal("diagnostic text was treated as a typed transport EOF")
	}
}
