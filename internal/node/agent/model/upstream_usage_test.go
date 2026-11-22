package model

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
)

const reportedUsageMissingDiagnostic = `error: code=502 reason="UPSTREAM_USAGE_MISSING" message="Upstream response is missing billable token usage; settlement was not completed" metadata=map[]`

func TestUpstreamUsageMissingClassification(t *testing.T) {
	envelope := func(code, reason, message string) string {
		raw, err := json.Marshal(map[string]any{"error": map[string]any{"code": code, "reason": reason, "message": message}})
		if err != nil {
			t.Fatal(err)
		}
		return string(raw)
	}
	for _, test := range []struct {
		name, body, errorType string
		status                int
		kind                  FailureKind
	}{
		{"reported diagnostic in generic error", envelope("api_error", "", reportedUsageMissingDiagnostic), "", 200, Unavailable},
		{"explicit code", envelope("UPSTREAM_USAGE_MISSING", "", "Settlement failed"), "", 200, Unavailable},
		{"explicit reason", envelope("api_error", "UPSTREAM_USAGE_MISSING", "Settlement failed"), "", 200, Unavailable},
		{"numeric gateway code", `{"error":{"code":502,"reason":"UPSTREAM_USAGE_MISSING","message":"Settlement failed"}}`, "", 200, Unavailable},
		{"nested response", fmt.Sprintf(`{"response":{"error":{"code":"api_error","message":%q}}}`, reportedUsageMissingDiagnostic), "", 200, Unavailable},
		{"top-level reason", `{"code":502,"reason":"UPSTREAM_USAGE_MISSING","message":"Settlement failed"}`, "", 200, Unavailable},
		{"provider error type", "", "UPSTREAM_USAGE_MISSING", 200, Unavailable},
		{"provider hint beside generic code", envelope("api_error", "", "Settlement failed"), "UPSTREAM_USAGE_MISSING", 200, Unavailable},
		{"unknown reason", envelope("api_error", "UPSTREAM_OTHER_FAILURE", "Settlement failed"), "", 200, InvalidResponse},
		{"usage text alone", envelope("api_error", "", "Upstream response is missing billable token usage; settlement was not completed"), "", 200, InvalidResponse},
		{"quoted diagnostic", envelope("api_error", "", "Previous diagnostic: "+reportedUsageMissingDiagnostic), "", 200, InvalidResponse},
		{"different embedded status", envelope("api_error", "", strings.Replace(reportedUsageMissingDiagnostic, "code=502", "code=400", 1)), "", 200, InvalidResponse},
		{"client rejection", envelope("api_error", "UPSTREAM_USAGE_MISSING", reportedUsageMissingDiagnostic), "", 400, InvalidRequest},
		{"authentication status", envelope("api_error", "", reportedUsageMissingDiagnostic), "", 401, Authentication},
		{"authentication code", envelope("invalid_api_key", "", reportedUsageMissingDiagnostic), "", 200, Authentication},
		{"context limit", envelope("context_length_exceeded", "", reportedUsageMissingDiagnostic), "", 200, ContextOverflow},
		{"quota", envelope("insufficient_quota", "", reportedUsageMissingDiagnostic), "", 200, Quota},
	} {
		t.Run(test.name, func(t *testing.T) {
			failure := Classify(&fantasy.ProviderError{StatusCode: test.status, ErrorType: test.errorType, ResponseBody: []byte(test.body)})
			if failure.Kind != test.kind || failure.Retryable() != (test.kind == Unavailable) || failure.status == nil || int(*failure.status) != test.status {
				t.Fatalf("gateway failure classification changed its cause or HTTP facts: %+v", failure)
			}
		})
	}
}

func TestSDKUpstreamUsageMissingRecovery(t *testing.T) {
	raw, err := json.Marshal(map[string]any{"type": "error", "error": map[string]any{"type": "api_error", "message": reportedUsageMissingDiagnostic}})
	if err != nil {
		t.Fatal(err)
	}
	errorEvent := "event: error\ndata: " + string(raw) + "\n\n"
	prefixes := map[string]string{
		"responses": "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"item\",\"call_id\":\"discarded\",\"name\":\"read\",\"arguments\":\"{}\"}}\n\n",
		"chat":      "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"discarded\",\"function\":{\"name\":\"read\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
		"anthropic": "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"discarded\",\"name\":\"read\",\"input\":{}}}\n\n" +
			"event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
	}
	for kind, prefix := range prefixes {
		for _, recovers := range []bool{true, false} {
			t.Run(fmt.Sprintf("%s/recovers_%t", kind, recovers), func(t *testing.T) {
				success := fixture(t, kind+"-text.sse")
				var requests atomic.Int32
				bodies := make(chan string, MainRetries+1)
				server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
					body, err := io.ReadAll(request.Body)
					if err != nil {
						t.Error(err)
					}
					bodies <- string(body)
					writer.Header().Set("Content-Type", "text/event-stream")
					if requests.Add(1) == 1 || !recovers {
						_, _ = io.WriteString(writer, prefix+errorEvent)
						return
					}
					_, _ = writer.Write(success)
				}))
				t.Cleanup(server.Close)
				language, err := newSDK(t.Context(), transportConfig{Kind: kind, Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
				if err != nil {
					t.Fatal(err)
				}
				waits, resets := 0, 0
				timer := &policyClock{beforeSleep: func() {
					waits++
					if requests.Load() != int32(waits) || resets != waits {
						t.Fatal("SDK resent the request before Sailry's reset and retry wait")
					}
				}}
				var attempts []Attempt
				var output []fantasy.StreamPart
				err = Run(t.Context(), language, requestCall(t, kind), Main, timer, func(attempt Attempt) error {
					attempts = append(attempts, attempt)
					return nil
				}, func(part fantasy.StreamPart) bool {
					output = append(output, part)
					return true
				}, func() { output = nil; resets++ })
				wantAttempts, wantFailures := MainRetries+1, MainRetries+1
				if recovers {
					wantAttempts, wantFailures = 2, 1
					if err != nil {
						t.Fatalf("a later successful response did not recover: %v", err)
					}
				} else if failure := Classify(err); failure == nil || failure.Kind != Unavailable || !failure.Exhausted || failure.WaitExhausted || failure.Message != reportedUsageMissingDiagnostic {
					t.Fatalf("an unresolved settlement failure was hidden or lost its retry limit: %+v", failure)
				}
				if int(requests.Load()) != wantAttempts || len(attempts) != wantAttempts || len(timer.delays) != wantAttempts-1 || resets != wantAttempts-1 {
					t.Fatalf("settlement recovery sent unexpected requests: requests=%d attempts=%d waits=%v resets=%d", requests.Load(), len(attempts), timer.delays, resets)
				}
				firstBody := <-bodies
				for range wantAttempts - 1 {
					if body := <-bodies; body != firstBody {
						t.Fatal("recovery rewrote the captured model request")
					}
				}
				for index, attempt := range attempts {
					if attempt.RequestID != attempts[0].RequestID || attempt.Index != uint64(index) || attempt.Status == nil || *attempt.Status != http.StatusOK {
						t.Fatalf("recovery lost request identity or the actual HTTP status: %+v", attempt)
					}
					if index < wantFailures {
						if attempt.Failure == nil || attempt.Failure.Kind != Unavailable || attempt.Failure.Message != reportedUsageMissingDiagnostic || !reflect.DeepEqual(attempt.Usage, canonical.Usage{}) {
							t.Fatalf("failed attempt lost its diagnostic or invented usage: %+v", attempt)
						}
					} else if attempt.Failure != nil {
						t.Fatalf("successful retry retained the old failure: %+v", attempt)
					}
				}
				text, finishes := "", 0
				for _, part := range output {
					if part.Type == fantasy.StreamPartTypeTextDelta {
						text += part.Delta
					}
					if part.Type == fantasy.StreamPartTypeFinish {
						finishes++
					}
					if recovers && part.Type == fantasy.StreamPartTypeToolCall {
						t.Fatal("a failed attempt's tool call survived its reset")
					}
				}
				if recovers && (finishes != 1 || text != "Read complete") || !recovers && finishes != 0 {
					t.Fatalf("HTTP 200 or a completed choice hid the upstream error: text=%q finishes=%d error=%v", text, finishes, err)
				}
			})
		}
	}
}
