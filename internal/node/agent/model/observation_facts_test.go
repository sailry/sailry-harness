package model

import (
	"bytes"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptrace"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/observation"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAttemptObservesFinalBodyAndHTTPPhases(t *testing.T) {
	t.Parallel()
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
	t.Parallel()
	timer := &policyClock{}
	ctx, capture := captureAttempt(t.Context(), fantasy.Call{}, timer, timer.Now())
	capturePrepared(ctx, []byte("Prompt without its transport envelope or attachments"), false)
	var attempt Attempt
	capture.apply(&attempt)
	if attempt.RequestBytes != nil || attempt.DispatchUs != nil || attempt.RequestSentUs != nil {
		t.Fatal("prompt-only input invented serialized transport measurements")
	}
}

func TestAdjacentResponsesObserveTheFinalSDKBody(t *testing.T) {
	t.Parallel()
	var history observation.ResponsesHistory
	session, firstTurn, nextTurn := protocol.NewSessionID(), protocol.NewTurnID(), protocol.NewTurnID()
	ctx, trace := history.Start(t.Context(), session, firstTurn)
	defer func() { trace.Clear() }()
	language, err := newSDK(ctx, transportConfig{Kind: "responses", Model: "fixture", Endpoint: "https://example.test/v1", Key: "fixture-key"})
	if err != nil {
		t.Fatal(err)
	}
	requests := 0
	var inputSizes [][]uint64
	language.(*sdkModel).client.Transport = responseTransport{stall: stallTimeout, base: pluginRoundTripper(func(request *http.Request) (*http.Response, error) {
		raw, err := io.ReadAll(request.Body)
		if err != nil {
			return nil, err
		}
		want := []byte(`"text":"serialized first"`)
		if requests == 2 {
			want = []byte(`"text":"serialized OTHER"`)
		}
		if request.URL.Path != "/v1/responses" || !bytes.Contains(raw, want) || bytes.Contains(raw, []byte("Read the file")) {
			t.Fatal("the SDK request did not exercise the serialized input override")
		}
		var sent struct {
			Input []json.RawMessage `json:"input"`
		}
		if err := json.Unmarshal(raw, &sent); err != nil {
			return nil, err
		}
		sizes := make([]uint64, len(sent.Input))
		for i, value := range sent.Input {
			sizes[i] = uint64(len(value))
		}
		inputSizes = append(inputSizes, sizes)
		stream := fixture(t, "responses-text.sse")
		if requests == 1 {
			stream = bytes.Replace(stream, []byte(`"cached_tokens":8`), []byte(`"cached_tokens":0`), 1)
		}
		requests++
		return &http.Response{StatusCode: http.StatusOK, Header: http.Header{"Content-Type": {"text/event-stream"}}, Body: io.NopCloser(bytes.NewReader(stream))}, nil
	})}
	var ids []string
	for i, first := range []string{"serialized first", "serialized first", "serialized OTHER"} {
		if i == 2 {
			history.Complete(ctx)
			trace.Clear()
			ctx, trace = history.Start(t.Context(), session, nextTurn)
		}
		call := requestCall(t, "responses")
		item := func(text string) map[string]any {
			return map[string]any{"type": "message", "role": "user", "content": []any{map[string]any{"type": "input_text", "text": text}}}
		}
		input := []any{item(first)}
		if i > 0 {
			input = append(input, item("new trailing input"))
		}
		call.ProviderOptions[parametersKey] = &callParameters{Body: map[string]any{"input": input, "prompt_cache_key": "fixture-session"}}
		var attempt Attempt
		if err := Run(ctx, language, call, Main, &policyClock{}, func(value Attempt) error {
			attempt = value
			return nil
		}, func(fantasy.StreamPart) bool { return true }, nil); err != nil {
			t.Fatal(err)
		}
		got := attempt.ResponsesBoundary
		cache := attempt.Usage.CacheRead
		if cache == nil || *cache != []uint64{8, 0, 8}[i] {
			t.Fatalf("request %d lost reported cache usage", i+1)
		}
		if i == 0 {
			if got != nil {
				t.Fatal("the first request must not have a predecessor")
			}
		} else {
			wantPreviousCache, wantShared, wantPreviousItems := uint64(8), uint64(1), uint64(1)
			wantPreviousTurn := protocol.TurnID{}
			wantSharedBytes := inputSizes[i][0]
			wantChange := protocol.ConversationResponsesInputChange{
				Kind:  protocol.ConversationResponsesInputUser,
				Added: protocol.ConversationResponsesInputSize{Items: 1, Bytes: inputSizes[i][1]},
			}
			if i == 2 {
				wantPreviousCache, wantShared, wantPreviousItems = 0, 0, 2
				wantPreviousTurn, wantSharedBytes = firstTurn, 0
				wantChange.Added.Bytes = inputSizes[i][0]
				wantChange.Removed = protocol.ConversationResponsesInputSize{Items: 1, Bytes: inputSizes[i-1][0]}
			}
			if got == nil || got.PreviousRequestID != ids[i-1] || got.PreviousAttempt != 1 || got.PreviousTurn != wantPreviousTurn || got.PreviousCachedInput == nil || *got.PreviousCachedInput != wantPreviousCache ||
				!got.InstructionsEqual || !got.ToolsEqual || !got.CacheOptionsEqual || !got.OtherFieldsEqual || got.SharedInputItems != wantShared || got.PreviousInputItems != wantPreviousItems || got.InputItems != 2 ||
				got.SharedInputBytes == nil || *got.SharedInputBytes != wantSharedBytes || len(got.InputChanges) != 1 || got.InputChanges[0] != wantChange || got.InputReorderIncomplete {
				t.Fatalf("request %d boundary = %+v", i+1, got)
			}
		}
		ids = append(ids, attempt.RequestID)
	}
	if requests != 3 {
		t.Fatalf("got %d HTTP requests, want 3", requests)
	}
}

func TestResponsesBoundaryRetriesRespectTransportReceipts(t *testing.T) {
	t.Parallel()
	for _, scenario := range []struct {
		name string
		gap  bool
	}{
		{name: "confirmed retry"},
		{name: "retry without headers", gap: true},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			var history observation.ResponsesHistory
			ctx, trace := history.Start(t.Context(), protocol.NewSessionID(), protocol.NewTurnID())
			defer trace.Clear()
			language, err := newSDK(ctx, transportConfig{Kind: "responses", Model: "fixture", Endpoint: "https://example.test/v1", Key: "fixture-key"})
			if err != nil {
				t.Fatal(err)
			}
			requests := 0
			stream := fixture(t, "responses-text.sse")
			language.(*sdkModel).client.Transport = responseTransport{stall: stallTimeout, base: pluginRoundTripper(func(request *http.Request) (*http.Response, error) {
				defer request.Body.Close()
				switch requests++; requests {
				case 2:
					if scenario.gap {
						return nil, io.ErrUnexpectedEOF
					}
					return &http.Response{StatusCode: http.StatusRequestTimeout, Header: http.Header{"Content-Type": {"application/json"}}, Body: io.NopCloser(bytes.NewBufferString(`{"error":{"code":"request_timeout","message":"Request timed out"}}`))}, nil
				case 4:
					// net/http can report more than one write for a logical
					// transport attempt, with no separate upstream receipts.
					writer := httptrace.ContextClientTrace(request.Context()).WroteRequest
					writer(httptrace.WroteRequestInfo{})
					writer(httptrace.WroteRequestInfo{})
				}
				return &http.Response{StatusCode: http.StatusOK, Header: http.Header{"Content-Type": {"text/event-stream"}}, Body: io.NopCloser(bytes.NewReader(stream))}, nil
			})}
			var attempts []Attempt
			for range 4 {
				err := Run(ctx, language, requestCall(t, "responses"), Main, &policyClock{}, func(attempt Attempt) error {
					attempts = append(attempts, attempt)
					return nil
				}, func(fantasy.StreamPart) bool { return true }, nil)
				if err != nil {
					t.Fatal(err)
				}
			}
			if len(attempts) != 5 || requests != 5 {
				t.Fatalf("got %d observations and %d requests, want 5 of each", len(attempts), requests)
			}
			first, retry := attempts[1], attempts[2]
			if first.RetryDelay == nil || first.RequestID != retry.RequestID || retry.Index != 1 {
				t.Fatalf("retry lost request identity: %+v, %+v", first, retry)
			}
			if scenario.gap {
				if first.ResponsesBoundary != nil || retry.ResponsesBoundary != nil {
					t.Fatal("diagnostic crossed an attempt without a response receipt")
				}
			} else {
				if got := first.ResponsesBoundary; got == nil || got.PreviousRequestID != attempts[0].RequestID || got.PreviousAttempt != 1 || got.PreviousCachedInput == nil || *got.PreviousCachedInput != 8 {
					t.Fatalf("first attempt lost its preceding request: %+v", got)
				}
				if got := retry.ResponsesBoundary; got == nil || got.PreviousRequestID != first.RequestID || got.PreviousAttempt != 1 || got.PreviousCachedInput != nil || got.SharedInputItems == 0 || len(got.InputChanges) != 0 {
					t.Fatalf("resend lost its preceding attempt or invented cache usage: %+v", got)
				}
			}
			if attempts[3].ResponsesBoundary != nil || attempts[4].ResponsesBoundary != nil {
				t.Fatal("diagnostic crossed an ambiguous transport replay")
			}
		})
	}
}
