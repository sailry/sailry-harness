package model

import (
	"bytes"
	"io"
	"net/http"
	"net/http/httptrace"
	"testing"

	"charm.land/fantasy"
)

func TestAttemptObservesFinalBodyAndHTTPPhases(t *testing.T) {
	for _, oauth := range []bool{false, true} {
		name := "API key"
		if oauth {
			name = "OAuth adapted body"
		}
		t.Run(name, func(t *testing.T) {
			language, err := newSDK(t.Context(), transportConfig{Kind: "anthropic", Model: "claude-opus-4-8", Endpoint: "https://example.test", Key: "fixture-key", OAuth: oauth})
			if err != nil {
				t.Fatal(err)
			}
			var sentBytes uint64
			var transport http.RoundTripper = responseTransport{stall: stallTimeout, base: pluginRoundTripper(func(request *http.Request) (*http.Response, error) {
				raw, err := io.ReadAll(request.Body)
				if err != nil {
					return nil, err
				}
				sentBytes = uint64(len(raw))
				if oauth && !bytes.Contains(raw, []byte(claudeIdentity)) {
					t.Fatal("fixture did not exercise the final OAuth rewrite")
				}
				capture := request.Context().Value(attemptCaptureKey{}).(*attemptCapture)
				var dispatched Attempt
				capture.apply(&dispatched)
				if dispatched.RequestBytes == nil || *dispatched.RequestBytes != sentBytes || dispatched.DispatchUs == nil || dispatched.RequestSentUs != nil || dispatched.ResponseUs != nil {
					t.Fatal("request preparation was not captured before transport I/O")
				}
				trace := httptrace.ContextClientTrace(request.Context())
				trace.WroteRequest(httptrace.WroteRequestInfo{})
				return &http.Response{StatusCode: http.StatusOK, Header: http.Header{"Content-Type": {"text/event-stream"}}, Body: io.NopCloser(bytes.NewReader(fixture(t, "anthropic-text.sse")))}, nil
			})}
			if oauth {
				transport = claudeTransport{base: transport}
			}
			language.(*sdkModel).client.Transport = transport
			var attempts []Attempt
			err = Run(t.Context(), language, requestCall(t, "anthropic"), Main, &policyClock{}, func(attempt Attempt) error {
				attempts = append(attempts, attempt)
				return nil
			}, func(fantasy.StreamPart) bool { return true }, nil)
			if err != nil || len(attempts) != 1 {
				t.Fatalf("request failed: attempts=%d error=%v", len(attempts), err)
			}
			attempt := attempts[0]
			if attempt.RequestBytes == nil || *attempt.RequestBytes != sentBytes || attempt.DispatchUs == nil || attempt.RequestSentUs == nil || attempt.ResponseUs == nil || attempt.FirstToken == nil {
				t.Fatalf("missing request measurements: %+v", attempt)
			}
			first := uint64(attempt.FirstToken.Sub(attempt.Started).Microseconds())
			if *attempt.DispatchUs >= *attempt.RequestSentUs || *attempt.RequestSentUs >= *attempt.ResponseUs || *attempt.ResponseUs >= first {
				t.Fatalf("preparation, request write, headers and output lost their boundaries: %+v", attempt)
			}
		})
	}
}

func TestPromptOnlyObservationLeavesTransportSizeUnknown(t *testing.T) {
	timer := &policyClock{}
	ctx, capture := captureAttempt(t.Context(), fantasy.Call{}, timer, timer.Now())
	capturePrepared(ctx, []byte("Prompt without its transport envelope or attachments"), false)
	var attempt Attempt
	capture.apply(&attempt)
	if attempt.RequestBytes != nil || attempt.DispatchUs != nil || attempt.RequestSentUs != nil {
		t.Fatal("prompt-only input invented serialized transport measurements")
	}
}
