package model

import (
	"encoding/json"
	"fmt"
	"net/http"
	"slices"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestFailureRetryHints(t *testing.T) {
	for _, test := range []struct {
		name       string
		status     int
		code       string
		kind       FailureKind
		overloaded bool
	}{
		{"rate limit", 429, "", RateLimited, true},
		{"stream rate limit", 200, "rate_limit_error", RateLimited, true},
		{"service unavailable", 503, "", Unavailable, false},
		{"service overloaded", 503, "overloaded_error", Unavailable, true},
		{"overload status", 529, "", Unavailable, true},
		{"overload code", 200, "overloaded_error", Unavailable, true},
		{"busy code", 200, "server_busy", Unavailable, true},
		{"timeout", 408, "", Unavailable, false},
		{"server error", 500, "", Unavailable, false},
		{"bad gateway", 502, "", Unavailable, false},
		{"gateway timeout", 504, "", Unavailable, false},
		{"quota before rate limit", 429, "insufficient_quota", Quota, false},
		{"quota before overload status", 529, "insufficient_quota", Quota, false},
		{"authentication before overload code", 401, "overloaded_error", Authentication, false},
		{"context before overload status", 529, "context_length_exceeded", ContextOverflow, false},
		{"invalid request", 400, "invalid_request", InvalidRequest, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			body, err := json.Marshal(map[string]any{"error": map[string]any{"code": test.code, "message": "request failed"}})
			if err != nil {
				t.Fatal(err)
			}
			failure := Classify(&fantasy.ProviderError{StatusCode: test.status, ResponseBody: body,
				ResponseHeaders: map[string]string{"rEtRy-AfTeR": "7", "Retry-After": "Wed, 21 Oct 2037 07:28:00 GMT", "X-Private": "not retry advice"}})
			if failure.Kind != test.kind || failure.overloaded != test.overloaded {
				t.Fatalf("classification: kind=%s overloaded=%t", failure.Kind, failure.overloaded)
			}
			if !slices.Equal(failure.retryAfter, []string{"Wed, 21 Oct 2037 07:28:00 GMT", "7"}) {
				t.Fatal("case-insensitive raw retry advice was lost or unrelated headers were retained")
			}
			if !failure.Retryable() && failure.overloaded {
				t.Fatal("retry advice made a permanent failure overloaded")
			}
		})
	}
	for _, status := range []int{200, 500, 503} {
		t.Run(fmt.Sprintf("overload type alongside a generic error code HTTP%d", status), func(t *testing.T) {
			failure := Classify(&fantasy.ProviderError{StatusCode: status, ResponseBody: []byte(`{"error":{"code":"api_error","type":"overloaded_error","message":"busy"}}`)})
			if failure.Kind != Unavailable || !failure.overloaded {
				t.Fatal("the primary error code hid explicit overload advice")
			}
		})
	}
}

func TestFailureRetryHintsCopyAndPrivacy(t *testing.T) {
	const privateAdvice = "private-retry-advice"
	delay := uint64(9000)
	original := &Failure{Kind: Unavailable, Message: "private-token failed", retryAfter: []string{privateAdvice}, retryAfterMs: &delay, overloaded: true, Exhausted: true, WaitExhausted: true}
	copy := Classify(fmt.Errorf("wrapped: %w", original), protocol.NewSecret("private-token"))
	original.retryAfter[0], delay = "changed", 1
	if copy.retryAfter[0] != privateAdvice || copy.retryAfterMs == nil || *copy.retryAfterMs != 9000 || !copy.overloaded || !copy.Exhausted || !copy.WaitExhausted {
		t.Fatal("reclassification aliased or lost retry metadata")
	}
	if copy.Message != "[redacted] failed" {
		t.Fatal("reclassification lost diagnostic redaction")
	}
	for _, value := range []any{copy, Fault(copy)} {
		encoded, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		if strings.Contains(string(encoded), privateAdvice) || strings.Contains(string(encoded), "private-token") {
			t.Fatal("private retry metadata or credentials escaped into serialized diagnostics")
		}
	}
	if strings.Contains(copy.Error(), privateAdvice) {
		t.Fatal("raw retry advice escaped into the failure message")
	}
}

func TestResponseHintsPreserveFailureKind(t *testing.T) {
	for _, kind := range []FailureKind{Authentication, ContextOverflow, Quota, InvalidRequest, InvalidResponse, Cancelled, Other} {
		t.Run(string(kind), func(t *testing.T) {
			failure := Classify(withResponseHints(&Failure{Kind: kind, Message: "failed", overloaded: true}, 503, http.Header{"retry-after": {"7", "11"}}))
			if failure.Kind != kind || failure.Retryable() || failure.overloaded || !slices.Equal(failure.retryAfter, []string{"7", "11"}) || failure.status == nil || *failure.status != 503 {
				t.Fatal("response advice changed a permanent failure or lost its HTTP facts")
			}
		})
	}
}

func TestFaultRecoveryExhaustionReason(t *testing.T) {
	for _, test := range []struct {
		name           string
		attempts, wait bool
		reason         protocol.ConversationRecoveryStop
	}{
		{"attempt limit", true, false, protocol.ConversationRecoveryStopAttemptsExhausted},
		{"wait budget", false, true, protocol.ConversationRecoveryStopWaitExhausted},
		{"wait takes precedence", true, true, protocol.ConversationRecoveryStopWaitExhausted},
	} {
		t.Run(test.name, func(t *testing.T) {
			fault := Fault(&Failure{Kind: Unavailable, Message: "busy", Exhausted: test.attempts, WaitExhausted: test.wait})
			if fault.Details == nil || fault.Details.Kind != "model_recovery" {
				t.Fatal("exhausted recovery became an ordinary request failure")
			}
			details, ok := fault.Details.Data.(protocol.FaultDetailsModelRecoveryPayload)
			if !ok || details.Reason != test.reason || fault.Message != "busy" {
				t.Fatal("recovery exhaustion lost its reason or underlying diagnostic")
			}
		})
	}
}
